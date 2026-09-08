// 字幕（音频监听）命令层 + 运行态。
//
// 与语音输入不同：这里没有"按住说话"的离散录音，而是一条常驻的监听会话——
// 采集系统混音、VAD 切句、送 ASR、推字幕。会话的启停/暂停/历史都归 SubtitleState 管。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio_capture::{AudioProcess, SessionConfig, SubtitleLine, SubtitleSession};
use crate::commands::AppState;
use crate::plugins::PluginManager;

/// 历史落盘文件名（app_data_dir 下）。
const HISTORY_FILE: &str = "subtitle_history.json";
/// 最多保留的会话数（决策 #5：最近 5 会话）。
const MAX_SESSIONS: usize = 5;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 毫秒时间戳 → 本地时间字符串。`with_date` 控制是否含日期。
fn fmt_ms(ms: u64, with_date: bool) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(dt) => {
            if with_date {
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            } else {
                dt.format("%H:%M:%S").to_string()
            }
        }
        None => "未知时间".to_string(),
    }
}

/// 运行中的会话句柄。
struct RunningSession {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    started_ts: u64,
    process: String,
}

/// 字幕监听全局状态（Tauri manage）。
pub struct SubtitleState {
    /// 当前会话（None = 未监听）
    running: Mutex<Option<RunningSession>>,
    /// 暂停标志（Alt+M 翻转；会话线程读取以决定是否转写）
    paused: Arc<AtomicBool>,
    /// 本轮会话已产出的字幕（emit 时同步追加；stop 时收编进 sessions 落盘）
    current: Arc<Mutex<Vec<SubtitleLine>>>,
    /// 历史会话（内存镜像，落盘在 HISTORY_FILE）
    sessions: Mutex<Vec<SubtitleSession>>,
}

impl SubtitleState {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(None),
            paused: Arc::new(AtomicBool::new(false)),
            current: Arc::new(Mutex::new(Vec::new())),
            sessions: Mutex::new(Vec::new()),
        }
    }
}

impl Default for SubtitleState {
    fn default() -> Self {
        Self::new()
    }
}

/// 事件名：暂停态变化（供浮窗/管理页同步"已暂停"提示）。
pub const SUBTITLE_PAUSED_EVENT: &str = "subtitle:paused";
/// 事件名：会话启停（供管理页按钮态刷新）。
pub const SUBTITLE_STATE_EVENT: &str = "subtitle:state";

// ── 历史落盘 ────────────────────────────────────

fn history_path(data_dir: &Path) -> PathBuf {
    data_dir.join(HISTORY_FILE)
}

fn load_history(data_dir: &Path) -> Vec<SubtitleSession> {
    let p = history_path(data_dir);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<SubtitleSession>>(&s).ok())
        .unwrap_or_default()
}

fn save_history(data_dir: &Path, sessions: &[SubtitleSession]) {
    let p = history_path(data_dir);
    if let Ok(json) = serde_json::to_string_pretty(sessions) {
        let _ = std::fs::write(p, json);
    }
}

/// 把内存历史首次访问时从盘载入（幂等：已加载则不覆盖）。
fn ensure_history_loaded(state: &SubtitleState, data_dir: &Path) {
    if let Ok(mut g) = state.sessions.lock() {
        if g.is_empty() {
            *g = load_history(data_dir);
        }
    }
}

// ── 命令 ────────────────────────────────────────

/// 枚举正在发声的进程（下拉候选）。
#[tauri::command]
#[cfg(windows)]
pub fn list_audio_processes() -> Result<Vec<AudioProcess>, String> {
    crate::audio_capture::processes::list_active_processes()
}

/// 枚举正在发声的进程（非 Windows 占位）。
#[tauri::command]
#[cfg(not(windows))]
pub fn list_audio_processes() -> Result<Vec<AudioProcess>, String> {
    Err("字幕监听仅在 Windows 平台可用".into())
}

/// 查询监听状态（前端初始化用）。
#[derive(serde::Serialize)]
pub struct SubtitleStatus {
    pub running: bool,
    pub paused: bool,
}

#[tauri::command]
pub fn subtitle_status(state: State<'_, SubtitleState>) -> SubtitleStatus {
    let running = state.running.lock().map(|g| g.is_some()).unwrap_or(false);
    SubtitleStatus {
        running,
        paused: state.paused.load(Ordering::Relaxed),
    }
}

/// 开始监听。快照设置 → 解析 ASR 插件 → 拉起会话线程。
#[tauri::command]
#[cfg(windows)]
pub fn start_audio_listener(
    app: AppHandle,
    state: State<'_, SubtitleState>,
    app_state: State<'_, AppState>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    // 已在运行则拒绝（避免双采集）
    if state.running.lock().map(|g| g.is_some()).unwrap_or(false) {
        return Err("字幕监听已在运行中".into());
    }

    let (plugin_id, language, sensitivity, target_process) = {
        let s = app_state
            .settings
            .read()
            .map_err(|e| format!("读取设置失败：{e}"))?;
        let pid = if !s.subtitle_asr_plugin.is_empty() {
            s.subtitle_asr_plugin.clone()
        } else {
            s.asr_plugin.clone()
        };
        (
            pid,
            s.subtitle_language.clone(),
            s.subtitle_vad_sensitivity.clone(),
            s.subtitle_target_process.clone(),
        )
    };

    if plugin_id.is_empty() {
        return Err("尚未选择 ASR 插件（请先安装并选择一个语音识别插件）".into());
    }

    let plugin = manager
        .get_asr(&plugin_id)
        .ok_or_else(|| format!("ASR 插件「{plugin_id}」未加载，请检查插件状态"))?;

    let cfg = SessionConfig {
        plugin_id,
        language,
        vad: crate::audio_capture::vad::VadConfig::from_sensitivity(&sensitivity),
    };

    // 重置本轮缓冲 + 暂停态
    if let Ok(mut g) = state.current.lock() {
        g.clear();
    }
    state.paused.store(false, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));

    let join = crate::audio_capture::session::spawn_session(
        app.clone(),
        plugin,
        cfg,
        state.paused.clone(),
        stop.clone(),
        state.current.clone(),
    );

    *state
        .running
        .lock()
        .map_err(|_| "状态锁毒化".to_string())? = Some(RunningSession {
        stop,
        join: Some(join),
        started_ts: now_ms(),
        process: target_process,
    });

    // 持久化开关 + 通知
    set_enabled_flag(&app_state, true);
    let _ = app.emit(SUBTITLE_STATE_EVENT, serde_json::json!({ "running": true, "paused": false }));
    Ok(())
}

/// 开始监听（非 Windows 占位）。
#[tauri::command]
#[cfg(not(windows))]
pub fn start_audio_listener(
    app: AppHandle,
    state: State<'_, SubtitleState>,
    app_state: State<'_, AppState>,
    manager: State<'_, PluginManager>,
) -> Result<(), String> {
    let _ = (&app, &state, &app_state, &manager);
    Err("字幕监听仅在 Windows 平台可用".into())
}

/// 停止监听：收编本轮字幕为一条会话记录并落盘（最近 5 条）。
#[tauri::command]
pub fn stop_audio_listener(
    app: AppHandle,
    state: State<'_, SubtitleState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let mut session = match state
        .running
        .lock()
        .map_err(|_| "状态锁毒化".to_string())?
        .take()
    {
        Some(s) => s,
        None => return Ok(()), // 本就未运行，幂等返回
    };

    session.stop.store(true, Ordering::Relaxed);
    if let Some(join) = session.join.take() {
        // 会话线程收尾会强制冲出末句，等它退出（含 join 采集线程）
        let _ = join.join();
    }

    // 收编本轮字幕
    let lines = state
        .current
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    if !lines.is_empty() {
        let record = SubtitleSession {
            started_ts: session.started_ts,
            ended_ts: now_ms(),
            process: session.process,
            lines,
        };
        ensure_history_loaded(&state, &app_state.data_dir);
        if let Ok(mut g) = state.sessions.lock() {
            g.insert(0, record);
            g.truncate(MAX_SESSIONS);
            save_history(&app_state.data_dir, &g);
        }
    }

    state.paused.store(false, Ordering::Relaxed);
    set_enabled_flag(&app_state, false);
    let _ = app.emit(SUBTITLE_STATE_EVENT, serde_json::json!({ "running": false, "paused": false }));
    Ok(())
}

/// 暂停/恢复切换（快捷键与前端按钮共用）。返回切换后的暂停态。
pub fn toggle_subtitle_pause(app: &AppHandle) -> bool {
    let state = match app.try_state::<SubtitleState>() {
        Some(s) => s,
        None => return false,
    };
    // 仅在运行时可暂停
    let running = state.running.lock().map(|g| g.is_some()).unwrap_or(false);
    if !running {
        return false;
    }
    let new_paused = !state.paused.load(Ordering::Relaxed);
    state.paused.store(new_paused, Ordering::Relaxed);
    let _ = app.emit(
        SUBTITLE_PAUSED_EVENT,
        serde_json::json!({ "paused": new_paused }),
    );
    new_paused
}

#[tauri::command]
pub fn set_subtitle_paused(app: AppHandle, paused: bool) -> bool {
    let state = match app.try_state::<SubtitleState>() {
        Some(s) => s,
        None => return false,
    };
    let running = state.running.lock().map(|g| g.is_some()).unwrap_or(false);
    if !running {
        return paused; // 未运行时不落地，返回期望值仅供前端一致显示
    }
    state.paused.store(paused, Ordering::Relaxed);
    let _ = app.emit(SUBTITLE_PAUSED_EVENT, serde_json::json!({ "paused": paused }));
    paused
}

/// 读取历史会话（回看）。
#[tauri::command]
pub fn get_subtitle_sessions(
    state: State<'_, SubtitleState>,
    app_state: State<'_, AppState>,
) -> Vec<SubtitleSession> {
    ensure_history_loaded(&state, &app_state.data_dir);
    state.sessions.lock().map(|g| g.clone()).unwrap_or_default()
}

/// 导出全部历史会话为纯文本（写前端给的路径）。
#[tauri::command]
pub fn export_subtitle_history(
    path: String,
    state: State<'_, SubtitleState>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    ensure_history_loaded(&state, &app_state.data_dir);
    let sessions = state.sessions.lock().map(|g| g.clone()).unwrap_or_default();
    let mut out = String::new();
    for (idx, s) in sessions.iter().enumerate() {
        let start = fmt_ms(s.started_ts, true);
        out.push_str(&format!("=== 会话 {} · {} · {} 句 ===\n", idx + 1, start, s.lines.len()));
        for line in &s.lines {
            let t = fmt_ms(line.ts, false);
            out.push_str(&format!("[{t}] {}\n", line.text));
        }
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| format!("导出失败：{e}"))
}

/// 更新 subtitle_enabled（写文件 + 内存，失败忽略不影响主流程）。
fn set_enabled_flag(app_state: &AppState, enabled: bool) {
    if let Ok(settings) = crate::storage::settings::update_setting(
        &app_state.data_dir,
        "subtitle_enabled",
        serde_json::json!(enabled),
    ) {
        if let Ok(mut g) = app_state.settings.write() {
            *g = settings;
        }
    }
}
