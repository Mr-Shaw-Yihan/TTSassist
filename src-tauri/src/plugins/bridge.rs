// 宿主能力桥（host bridge）：插件→宿主反向调用的通用基建。
//
// 宿主加载声明了 requires_host_bridge: true 且导出 va_plugin_attach_host 的插件时，
// 为其构造一张 VaHostServices 函数指针表并注入。表中每个能力只做通用包装：
// - 复用既有内部函数（收藏播放与快捷键同逻辑、合成走 generate_tts_impl 等）；
// - 确缺的通用能力补最小实现（playback:stop 事件 / 播放态聚合），不写任何
//   特定插件的业务逻辑。
//
// 播放态来源：主窗前端是实际播放者（HTMLAudioElement），开始/停止时发
// playback:started / playback:stopped 通用事件（与 playback:play-last 同族），
// 本模块监听聚合后供 get_state 查询与 subscribe_events 推送。
//
// 设计文档：doc/移动端遥控器设计.md §二（能力清单与内存约定）。

use std::ffi::{c_char, CString, CStr};
use std::os::raw::c_void;
use std::sync::RwLock;

use tauri::{AppHandle, Emitter, Manager};

use plugin_api::{VaHostCtx, VaHostEventCallback, VaHostServices, VA_HOST_SERVICES_VERSION};

use super::manifest::PluginManifest;
use crate::commands::mic::MicPlayback;
use crate::plugins::PluginManager;

/// 每个被注入插件的私有上下文（Box::leak 常驻——dll 加载后不卸载，进程退出统一回收）
struct BridgeCtx {
    plugin_id: String,
    app: AppHandle,
}

impl BridgeCtx {
    unsafe fn from_raw(ctx: VaHostCtx) -> Option<&'static Self> {
        (ctx as *const BridgeCtx).as_ref()
    }
}

/// 一个插件的事件订阅（回调指针由插件保证在其 dll 存活期间有效）
struct Subscriber {
    #[allow(dead_code)]
    plugin_id: String,
    cb: VaHostEventCallback,
    user_data: *mut c_void,
}

struct BridgeInner {
    /// 当前扬声器播放的音频相对路径（主窗上报；None = 空闲）
    playing_path: Option<String>,
    /// 是否有合成进行中（generate_tts_impl 置位）
    synthesizing: bool,
    subscribers: Vec<Subscriber>,
}

/// 宿主能力桥（Tauri State）。须在 PluginManager::load_all 之前 manage，
/// 插件 attach 期间即可调用 get_state / subscribe_events。
pub struct HostBridge {
    inner: RwLock<BridgeInner>,
}

// Subscriber 持插件提供的裸指针，由插件保证 Send+Sync（回调可任意线程触发）
unsafe impl Send for HostBridge {}
unsafe impl Sync for HostBridge {}

impl HostBridge {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(BridgeInner {
                playing_path: None,
                synthesizing: false,
                subscribers: Vec::new(),
            }),
        }
    }

    /// 注册前端播放事件监听（主窗 playback:started / playback:stopped → 聚合 + 推送）。
    /// 在宿主 setup 中调用一次；监听随进程存活，无需退订。
    pub fn setup_playback_listeners(&self, app: &AppHandle) {
        use tauri::Listener;
        let app_started = app.clone();
        app.listen_any("playback:started", move |event| {
            // payload = 音频相对路径的 JSON 串（主窗 playAudio 发出）
            let path = serde_json::from_str::<String>(event.payload()).ok();
            let Some(path) = path else { return };
            if let Some(bridge) = app_started.try_state::<HostBridge>() {
                if let Ok(mut inner) = bridge.inner.write() {
                    inner.playing_path = Some(path);
                }
            }
            Self::broadcast(&app_started, r#"{"type":"playback_changed"}"#);
        });
        let app_stopped = app.clone();
        app.listen_any("playback:stopped", move |_| {
            if let Some(bridge) = app_stopped.try_state::<HostBridge>() {
                if let Ok(mut inner) = bridge.inner.write() {
                    inner.playing_path = None;
                }
            }
            Self::broadcast(&app_stopped, r#"{"type":"playback_changed"}"#);
        });
    }

    /// 向全部订阅插件推送事件 JSON（回调在宿主线程同步触发，插件须自行保证线程安全）
    fn broadcast(app: &AppHandle, event_json: &str) {
        let Some(bridge) = app.try_state::<HostBridge>() else { return };
        let Ok(inner) = bridge.inner.read() else { return };
        let c = match CString::new(event_json) {
            Ok(c) => c,
            Err(_) => return,
        };
        for sub in &inner.subscribers {
            unsafe { (sub.cb)(sub.user_data, c.as_ptr()) };
        }
    }

    /// 数据变化转发入口（sync::notify_changed 钩子调用）：收藏/设置变化推给订阅插件
    pub fn notify_data_changed(app: &AppHandle, event: &str) {
        let json = match event {
            crate::sync::EVENT_FAVORITE_CHANGED => r#"{"type":"favorites_changed"}"#,
            crate::sync::EVENT_SETTINGS_CHANGED => r#"{"type":"settings_changed"}"#,
            _ => return,
        };
        Self::broadcast(app, json);
    }

    /// 标记合成进行中（generate_tts_impl 调用；结束由 Drop 守卫复位）
    pub fn set_synthesizing_flag(app: &AppHandle, on: bool) {
        if let Some(bridge) = app.try_state::<HostBridge>() {
            if let Ok(mut inner) = bridge.inner.write() {
                inner.synthesizing = on;
            }
            if !on {
                Self::broadcast(app, r#"{"type":"state_changed"}"#);
            }
        }
    }

    /// 为一个插件构造能力表并注入（加载成功后由 PluginManager 调用）。
    /// ctx 泄漏常驻（dll 不卸载）；返回 false 表示插件未导出 attach 符号。
    pub fn attach_plugin(
        &self,
        app: &AppHandle,
        plugin_id: &str,
        attach: plugin_api::VaPluginAttachHostFn,
    ) -> bool {
        let ctx = Box::leak(Box::new(BridgeCtx {
            plugin_id: plugin_id.to_string(),
            app: app.clone(),
        }));
        let table = Box::leak(Box::new(VaHostServices {
            version: VA_HOST_SERVICES_VERSION,
            ctx: ctx as *mut BridgeCtx as VaHostCtx,
            free_string: br_free_string,
            list_favorites: br_list_favorites,
            play_favorite: br_play_favorite,
            stop_playback: br_stop_playback,
            synthesize: br_synthesize,
            toggle_mic_send: br_toggle_mic_send,
            play_last: br_play_last,
            get_state: br_get_state,
            subscribe_events: br_subscribe_events,
            set_own_config: br_set_own_config,
            confirm_dialog: br_confirm_dialog,
        }));
        unsafe { attach(table as *const VaHostServices) };
        eprintln!("宿主能力桥已注入插件「{plugin_id}」");
        true
    }
}

// ── 能力实现（全部为通用包装，不含任何插件特有逻辑）──────────────

/// 读入参 C 字符串；NULL/非法 UTF-8 转中文错误
unsafe fn read_arg<'a>(ptr: *const c_char, what: &str) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("{what}不能为空"));
    }
    CStr::from_ptr(ptr).to_str().map_err(|_| format!("{what}不是合法 UTF-8"))
}

/// 把 String 写到 out（宿主分配，插件经 free_string 归还）
fn write_out(out: *mut *mut c_char, s: String) {
    if !out.is_null() {
        if let Ok(c) = CString::new(s) {
            unsafe { *out = c.into_raw() };
        }
    }
}

/// 收藏播放（与收藏快捷键回调同逻辑）：麦克风开启时发虚拟麦克风 + 发事件让主窗播扬声器。
fn play_favorite_by_id(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app
        .try_state::<crate::commands::AppState>()
        .ok_or("宿主状态未就绪")?;
    let favorites = crate::storage::favorites::load_favorites(&state.data_dir);
    let fav = favorites
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| format!("收藏「{id}」不存在"))?;

    // 发麦克风（若全局开关开启且配置了设备）——与 hotkey.rs 收藏快捷键一致
    let (enabled, device, volume) = match state.settings.read() {
        Ok(s) => (s.mic_send_enabled, s.mic_output_device.clone(), s.mic_playback_volume),
        Err(_) => (false, String::new(), 1.0),
    };
    if enabled && !device.is_empty() {
        if let Some(mic) = app.try_state::<MicPlayback>() {
            mic.play(state.data_dir.join(&fav.audio_path), device, volume);
        }
    }
    // emit 事件让前端主窗播扬声器
    let _ = app.emit("favorite:play", fav.audio_path);
    Ok(())
}

/// 状态快照 JSON：{"mic_send","playing_id","synthesizing"}。
/// playing_id = 当前播放音频对应的收藏 id（无收藏匹配时 null）
fn state_json(app: &AppHandle) -> String {
    let mic_send = app
        .try_state::<crate::commands::AppState>()
        .map(|s| s.settings.read().map(|g| g.mic_send_enabled).unwrap_or(false))
        .unwrap_or(false);

    let (playing_path, synthesizing) = app
        .try_state::<HostBridge>()
        .and_then(|b| b.inner.read().ok().map(|i| (i.playing_path.clone(), i.synthesizing)))
        .unwrap_or((None, false));

    let playing_id = playing_path.as_deref().and_then(|path| {
        let state = app.try_state::<crate::commands::AppState>()?;
        let favs = crate::storage::favorites::load_favorites(&state.data_dir);
        favs.into_iter().find(|f| f.audio_path == path).map(|f| f.id)
    });

    serde_json::json!({
        "mic_send": mic_send,
        "playing_id": playing_id,
        "synthesizing": synthesizing,
    })
    .to_string()
}

/// set_own_config 实现：回写自身 display 字段（仅 display 类型生效）→ 落盘 →
/// 注入环境变量 → 广播 settings:changed（设置面板刷新）。
fn set_own_config_impl(app: &AppHandle, plugin_id: &str, key: &str, value: &str) -> Result<(), String> {
    let plugins = app
        .try_state::<PluginManager>()
        .ok_or("插件管理器未就绪")?;
    let manifest: PluginManifest = plugins.manifest_of(plugin_id).ok_or("插件未安装")?;
    let decl = manifest.config.as_ref().ok_or("插件没有声明配置项")?;
    let field = decl
        .fields
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| format!("插件未声明配置字段「{key}」"))?;
    if field.r#type != "display" {
        return Err("能力桥只能回写 display 类型的配置字段".to_string());
    }

    let state = app
        .try_state::<crate::commands::AppState>()
        .ok_or("宿主状态未就绪")?;
    let entry = {
        let mut settings = state
            .settings
            .write()
            .map_err(|e| format!("读取设置失败: {e}"))?;
        settings
            .plugin_config
            .entry(plugin_id.to_string())
            .or_default()
            .insert(key.to_string(), value.to_string());
        settings.plugin_config.get(plugin_id).cloned().unwrap_or_default()
    };
    let snapshot = state
        .settings
        .read()
        .map_err(|e| format!("读取设置失败: {e}"))?
        .clone();
    crate::storage::settings::save_settings(&state.data_dir, &snapshot)
        .map_err(|e| format!("保存设置失败: {e}"))?;
    super::config::inject_manifest(&manifest, Some(&entry));
    crate::sync::notify_changed(app, crate::sync::EVENT_SETTINGS_CHANGED);
    Ok(())
}

// ── C ABI 能力函数（表内条目）────────────────────────────

unsafe extern "C" fn br_free_string(_ctx: VaHostCtx, ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

unsafe extern "C" fn br_list_favorites(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    let list = bc
        .app
        .try_state::<crate::commands::AppState>()
        .map(|s| crate::storage::favorites::load_favorites(&s.data_dir))
        .unwrap_or_default();
    // 只回元数据（id/备注/时间/快捷键），不含音频路径
    let items: Vec<serde_json::Value> = list
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "note": f.note,
                "created_at": f.created_at,
                "hotkey": f.hotkey,
            })
        })
        .collect();
    write_out(out_json, serde_json::Value::Array(items).to_string());
    plugin_api::VA_OK
}

unsafe extern "C" fn br_play_favorite(
    ctx: VaHostCtx,
    id: *const c_char,
    out_err: *mut *mut c_char,
) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    let result = read_arg(id, "收藏 id").and_then(|id| play_favorite_by_id(&bc.app, id));
    match result {
        Ok(()) => plugin_api::VA_OK,
        Err(e) => {
            write_out(out_err, e);
            plugin_api::VA_ERR
        }
    }
}

unsafe extern "C" fn br_stop_playback(ctx: VaHostCtx) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    // 通用停止：主窗监听 playback:stop 停扬声器（并回报 playback:stopped），
    // 虚拟麦克风侧直接停
    let _ = bc.app.emit("playback:stop", ());
    if let Some(mic) = bc.app.try_state::<MicPlayback>() {
        mic.stop();
    }
    plugin_api::VA_OK
}

unsafe extern "C" fn br_synthesize(
    ctx: VaHostCtx,
    text: *const c_char,
    out_err: *mut *mut c_char,
) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    let result = read_arg(text, "文本").and_then(|text| {
        if text.trim().is_empty() {
            return Err("文本不能为空".to_string());
        }
        // 阻塞执行现有合成管线（引擎分发 + 消息记录 + 麦克风）。
        // 调用线程来自插件，不在宿主异步运行时内，block_on 安全。
        tauri::async_runtime::block_on(crate::commands::tts::generate_tts_impl(&bc.app, text))
            .map(|msg: crate::storage::types::Message| {
                // 扬声器播放：主窗监听 playback:play（与后端麦克风避免双份）
                let _ = bc.app.emit("playback:play", msg.audio_path);
            })
    });
    match result {
        Ok(()) => plugin_api::VA_OK,
        Err(e) => {
            write_out(out_err, e);
            plugin_api::VA_ERR
        }
    }
}

unsafe extern "C" fn br_toggle_mic_send(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    // 与快捷键/悬浮球菜单共用同一入口（翻转 → 持久化 → 广播）
    crate::hotkey::toggle_mic_send(&bc.app);
    let on = bc
        .app
        .try_state::<crate::commands::AppState>()
        .map(|s| s.settings.read().map(|g| g.mic_send_enabled).unwrap_or(false))
        .unwrap_or(false);
    write_out(out_json, serde_json::json!({ "mic_send": on }).to_string());
    plugin_api::VA_OK
}

unsafe extern "C" fn br_play_last(ctx: VaHostCtx) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    // 与「播放最近一条消息」快捷键同通道
    let _ = bc.app.emit("playback:play-last", ());
    plugin_api::VA_OK
}

unsafe extern "C" fn br_get_state(ctx: VaHostCtx, out_json: *mut *mut c_char) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    write_out(out_json, state_json(&bc.app));
    plugin_api::VA_OK
}

unsafe extern "C" fn br_subscribe_events(
    ctx: VaHostCtx,
    cb: VaHostEventCallback,
    user_data: *mut c_void,
) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    let Some(bridge) = bc.app.try_state::<HostBridge>() else { return plugin_api::VA_ERR };
    let ok = bridge
        .inner
        .write()
        .map(|mut inner| {
            inner.subscribers.push(Subscriber {
                plugin_id: bc.plugin_id.clone(),
                cb,
                user_data,
            });
        })
        .is_ok();
    if ok {
        plugin_api::VA_OK
    } else {
        plugin_api::VA_ERR
    }
}

unsafe extern "C" fn br_set_own_config(
    ctx: VaHostCtx,
    key: *const c_char,
    value: *const c_char,
    out_err: *mut *mut c_char,
) -> i32 {
    let Some(bc) = BridgeCtx::from_raw(ctx) else { return plugin_api::VA_ERR };
    let result = (|| {
        let key = read_arg(key, "配置 key")?;
        let value = read_arg(value, "配置 value")?;
        set_own_config_impl(&bc.app, &bc.plugin_id, key, value)
    })();
    match result {
        Ok(()) => plugin_api::VA_OK,
        Err(e) => {
            write_out(out_err, e);
            plugin_api::VA_ERR
        }
    }
}

unsafe extern "C" fn br_confirm_dialog(
    ctx: VaHostCtx,
    title: *const c_char,
    body: *const c_char,
) -> i32 {
    if BridgeCtx::from_raw(ctx).is_none() {
        return -1;
    }
    let title = match read_arg(title, "标题") {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    let body = match read_arg(body, "内容") {
        Ok(s) => s.to_string(),
        Err(_) => return -1,
    };
    // 原生是/否弹窗：Win32 MessageBoxW + 系统置顶（tauri-plugin-dialog 的无父级
    // MessageBox 在宿主退居后台时会被前台窗口完全遮住）；本函数由插件后台线程
    // （spawn_blocking）调用，MessageBoxW 原生支持任意线程。
    match crate::win32::confirm_yes_no(&title, &body) {
        Some(true) => 1,
        Some(false) => 0,
        None => -1,
    }
}

// set_synthesizing 的公开薄包装（供 generate_tts_impl 的守卫调用；
// 独立函数避免命令模块依赖内部细节）
pub fn set_synthesizing(app: &AppHandle, on: bool) {
    HostBridge::set_synthesizing_flag(app, on);
}
