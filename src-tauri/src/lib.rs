// VoiceAssist Tauri 后端入口。
// 多窗口架构：main（主窗）+ quick_input（浮窗），跨窗口同步事件。

#[macro_use]
pub mod logging;

pub mod asr;
pub mod audio_capture;
pub mod commands;
pub mod diag_redact;
pub mod hotkey;
pub mod perf;
pub mod plugins;
pub mod proc;
pub mod remote;
pub mod storage;
pub mod sync;
pub mod tray;
pub mod tts;
pub mod win32;
pub mod window_size;

use std::sync::Mutex;
use tauri::Manager;
use crate::commands::AppState;
use crate::hotkey::{FavoriteHotkeys, HotkeyState};
use crate::storage::types::Settings;

/// 解析三根（配置 / 内容 / 插件），路径来源收口到 `storage::paths`。
///
/// 内容根默认 = `<应用目录>\data`，用户可用 `VA_STORAGE_DIR` 或 `storage.json` 指针自选；
/// `<exe>\data` 只是 1.9.0 测试版的历史来源（见 A3 回迁）。
fn resolve_storage_paths(app: &tauri::AppHandle) -> storage::paths::StoragePaths {
    use std::path::PathBuf;
    use storage::paths::*;
    // 存储契约要拦的是“业务代码自己向系统要数据目录”，而这里就是那个唯一解析入口本身
    //（下游一律拿 StoragePaths，不再自己调）。注意豁免只认本行与紧邻上一行，
    //  所以 `storageok:` 必须写在调用正上方，不能放在多行说明的第一行。
    // storageok: 全项目唯一合法的取应用目录处
    let app_root = app.path().app_data_dir().expect("无法获取应用目录");
    let exe_dir = std::env::current_exe()
        .expect("无法获取 exe 路径")
        .parent()
        .expect("exe 无父目录")
        .to_path_buf();

    // 新布局：配置进 `<应用目录>\config`。首次启动时把旧布局（直接放应用目录根）
    // 的配置文件搬进去；搬不动就继续用旧根，保证设置始终读得到。
    let config_root = ensure_config_root(&app_root);

    let env_dir = std::env::var("VA_STORAGE_DIR").ok().map(PathBuf::from);
    // 指针优先读 config\storage.json；旧布局的指针可能还在应用目录根。
    // 指向应用目录根的指针 = 旧默认位置（不是用户自选）→ 忽略，改走新默认 data\。
    let pointer = read_pointer(&config_root.join(POINTER_FILE_NAME))
        .or_else(|| read_pointer(&app_root.join(POINTER_FILE_NAME)))
        .filter(|p| *p != app_root);
    let local_fallback = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|a| PathBuf::from(a).join("com.voiceassist.app").join("data"))
        .unwrap_or_else(|| exe_dir.join("data-fallback"));

    // 内容根默认 = `<应用目录>\data`：数据不再放安装目录，覆盖升级/卸载不会连带删掉。
    // 用户自选位置走 storage.json 指针；装到不可写目录时回退 LOCALAPPDATA 并置 fallback。
    let (content_root, fallback) = decide_content_root(
        env_dir,
        pointer,
        Some(app_root.join(DATA_SUBDIR_NAME)),
        exe_dir.join("data"),
        local_fallback,
        &probe_writable,
    );

    // 插件代码根：VA_PLUGINS_DIR > exe 同级 plugins/（可写）> `<内容根>\plugins` 回退。
    // Program Files 场景下安装目录不可写，不回退会导致装/更新插件失败。
    let (plugins_root, plugins_fallback) = decide_plugins_root(
        std::env::var("VA_PLUGINS_DIR").ok().map(PathBuf::from),
        exe_dir.join("plugins"),
        content_root.clone(),
        &probe_writable,
    );

    let mut paths = storage::paths::StoragePaths::from_dirs(
        app_root,
        config_root,
        content_root,
        plugins_root,
        fallback,
    );
    paths.plugins_fallback = plugins_fallback;
    // 1.9.0 测试版的旧数据源（<exe>\data）。只在这里取一次，业务代码不碰 current_exe；
    // 与内容根相同时报告侧会忽略（表示没有“另一处旧数据”）。
    paths.legacy_exe_data = Some(exe_dir.join("data"));
    if let Err(e) = paths.ensure_layout() {
        log_warn!("创建存储目录失败: {e}");
    }
    if fallback {
        log_warn!(
            "内容数据回退到 {}（安装目录不可写或指定位置失效），设置页需向用户说明",
            paths.content_root.display()
        );
    }
    if plugins_fallback {
        log_warn!(
            "插件目录回退到 {}（安装目录不可写），设置页需向用户说明",
            paths.plugins_root.display()
        );
    }
    paths
}

/// 首次运行迁移：把旧 APPDATA 下的插件复制到新位置（exe 同级 plugins/）。
/// 旧目录保留不删（安全起见）；新位置已有 registry.json 则跳过（避免覆盖）。
fn migrate_plugins_if_needed(config_dir: &std::path::Path, plugins_root: &std::path::Path) {
    let old_root = config_dir.join("plugins");
    let old_registry = old_root.join("registry.json");
    let new_registry = plugins_root.join("registry.json");

    // 旧位置没插件，或新位置已有注册表 → 无需迁移
    if !old_registry.exists() || new_registry.exists() {
        return;
    }

    log_info!("检测到旧位置插件，正在迁移: {} → {}", old_root.display(), plugins_root.display());
    if let Err(e) = copy_dir_all(&old_root, plugins_root) {
        log_error!("插件迁移失败（不影响启动，可手动复制）: {e}");
    } else {
        log_info!("插件迁移完成。旧目录保留在 {}，确认无误后可手动删除。", old_root.display());
    }
}

/// 递归复制目录（迁移用）
pub(crate) fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}

/// 显示并聚焦主窗口（从浮窗按钮调用），同时隐藏浮窗（避免两窗同现）
#[tauri::command]
fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.show().map_err(|e| format!("{e}"))?;
        win.set_focus().map_err(|e| format!("{e}"))?;
    }
    if let Some(floating) = app.get_webview_window("quick_input") {
        let _ = floating.hide();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // [perf] startup 起点（T2）：run() 进入 → 前端首帧（perf_startup_done 命令回执）
    let _ = crate::perf::START.set(std::time::Instant::now());
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // 已有实例被再次启动：把已存在的主窗拉前台
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            // 三根在此一次解析；下面所有路径都从 paths 来（存储契约）
            let paths = resolve_storage_paths(app.handle());
            app.manage(paths.clone());
            let (config_dir, content_dir) = (paths.config_root.clone(), paths.content_root.clone());
            storage::ensure_data_dirs(&content_dir)?;

            // 兼容迁移：老设置里的内置引擎标识 → 对应插件 id（幂等，直接改文件）
            // settings.json 属配置 → 走配置根
            plugins::migrate_legacy_engine(&config_dir);

            let settings: Settings = storage::settings::load_settings(&config_dir);

            // 诊断日志（支持模式）：默认关仅 stderr；设置或 env VA_DIAG_LOG 命中时
            // 同时落 `<内容根>/logs/app.log`（仅本地、不含用户合成文本，启动超限自动轮转）
            {
                let env_on = std::env::var("VA_DIAG_LOG")
                    .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
                    .unwrap_or(false);
                logging::init(&paths.logs_dir(), settings.diagnostics_log_enabled || env_on);
                // T-G：release 下前端首帧早于本 init，startup 埋点若已暂存则在此补写落盘
                crate::perf::drain_startup_pending();
            }

            // 主窗以 visible=false 创建：先按记忆尺寸定形、铺好首帧底色，
            // **显示推迟到前端首帧就绪**（main_window_ready），否则会在
            // “show → 页面加载完”之间露出 WebView2 白底（先闪一帧日间）。
            window_size::prepare_main_window(app.handle(), &settings);

            // asset: 协议必须放行内容根，否则 `<audio src>` 会被作用域直接拒掉，
            // 症状是“点了没声、按钮还卡在播放中”——翻默认后音频已经不住在 $APPDATA 里了
            //（真机验收当时就是这么暴出来的，单测与构建皆绿，只有跑起来才发现）。
            // 为什么不能只改 tauri.conf.json：内容根是 `VA_STORAGE_DIR` > storage.json 指针 >
            // `<exe>\data` > 不可写时回退 LOCALAPPDATA 动态定出来的，静态配置表达不了“它今天是谁”。
            // $APPDATA/** 仍保留在配置里：用急救程序把指针指回配置根（旧布局）时靠它。
            if let Err(e) = app.asset_protocol_scope().allow_directory(&content_dir, true) {
                log_warn!("放行内容根的 asset 访问失败，历史音频可能无法回放: {e}");
            }

            // 首启数据搬家（幂等，1.9+ 每版启动都跑）：放在日志初始化之后、AppState 之前。
            // 这个顺序是关键：会写内容根的命令全部依赖 AppState，此刻还没 manage，
            // 所以不存在“后台正搬着、前端同时往 messages.json 写新数据”的竞态。
            // 选“自动搬完只弹提示”而不是“等用户点”：升级后看到空聊天列表，
            // 不懂技术的用户第一反应是“软件把数据弄没了”，甚至直接卸载重装。
            app.manage(commands::storage::MigrationNotice::default());
            match storage::migrate::detect_in(&paths) {
                storage::migrate::StorageState::Migratable => {
                    // 旧布局的「我的数据」直接在应用目录根（配置根现在是 config\\，不是旧位置）
                    let legacy = paths.app_root.clone();
                    let content = paths.content_root.clone();
                    let stamp = chrono::Local::now().format("%Y%m%d").to_string();
                    match storage::migrate::migrate_items(&legacy, &content, &stamp) {
                        Ok(r) => {
                            let mb = r.bytes as f64 / 1024.0 / 1024.0;
                            log_info!("自动迁移完成：{} 项 / {:.1} MB，原件在 {}", r.moved, mb, r.backup.display());
                            app.state::<commands::storage::MigrationNotice>().set(format!(
                                "已把旧位置的数据搬到 {}（共 {} 项，约 {:.1} MB）。原件保留在 {}，需要时可以退回。",
                                content.display(),
                                r.moved,
                                mb,
                                r.backup.display()
                            ));
                        }
                        // 搬不动就退回手动：旧数据一条没动，设置-存储那张卡片仍在
                        Err(e) => log_warn!("自动迁移未完成（可在设置页「存储」里手动重试）：{e}"),
                    }
                }
                storage::migrate::StorageState::Conflict => {
                    log_warn!("新旧两处都有数据，需用户在设置页选择处置（不静默二选一）")
                }
                other => log_info!("存储状态：{:?}", other),
            }

            // 1.9.0 测试版曾把「我的数据」放在安装目录：升级后自动迁回默认位置，
            // 否则用户看到空聊天列表会以为数据丢了。用户自选过位置时不回迁。
            if let Some(text) = commands::storage::pull_back_legacy_if_needed(&paths) {
                app.state::<commands::storage::MigrationNotice>().set(text);
            }

            // 克隆样本通用环境变量：宿主只把「已导入样本的绝对路径」告诉插件，
            // 格式/大小/用法全部由插件自决（基座不认识任何引擎，见 AGENTS.md 基座合同）。
            // API Key 不再在此硬编码注入：改由各插件 manifest 的 config 声明，
            // 统一走下方 plugin_manager.inject_config_env。
            {
                let sample_abs: Option<std::path::PathBuf> = if settings.clone_voice_path.is_empty() {
                    None
                } else {
                    let p = content_dir.join(&settings.clone_voice_path);
                    p.exists().then_some(p)
                };
                match sample_abs {
                    Some(p) => crate::commands::clone_voice::announce_clone_sample(&p),
                    None => crate::commands::clone_voice::clear_clone_sample(),
                }
            }

            // 宿主能力桥：先于插件加载 manage——插件 attach 期间即可调用
            // get_state / subscribe_events（桥的播放态聚合监听也在此注册）
            let host_bridge = plugins::HostBridge::new();
            host_bridge.setup_playback_listeners(app.handle());
            app.manage(host_bridge);

            // AppState / MicPlayback / StreamPlayer 提前 manage：能力桥包装的合成、
            // 收藏播放、停止播放等能力依赖二者，插件 attach 时必须已就绪
            app.manage(AppState::new(
                config_dir.clone(),
                content_dir.clone(),
                settings.clone(),
            ));
            app.manage(crate::commands::mic::MicPlayback::spawn());
            // 流式 TTS 播放线程（插件 WS 逐块 PCM → 边收边播）
            app.manage(crate::commands::tts_stream::StreamPlayer::spawn());

            // 启动即"热启动"虚拟麦克风：播一小段内置音频把 Windows 冷音频端点提前激活
            //（与手动“测试”同一播放路径），根治“冷启动首发语音进不了麦、对方听不见”。
            // 热启目标 = 发送实际会走的那根（mic_output_device），保证“暖的就是发的”，
            // 不再出现暖一根、发另一根的分叉（真机踩到：发送选了没有采集端的 16ch cable）。
            // 用户还没选设备时，回退自动认那根“有配对 CABLE Output 采集端”的渲染设备。
            if settings.mic_send_enabled {
                let chosen = settings.mic_output_device.trim();
                let dev = if !chosen.is_empty() {
                    Some(chosen.to_string())
                } else {
                    crate::commands::mic::find_virtual_cable_output_device()
                };
                if let Some(dev) = dev {
                    let vol = settings.mic_playback_volume;
                    app.state::<crate::commands::mic::MicPlayback>().warm(dev, vol);
                }
            }

            // 插件代码根跟随安装位置（阶段 22 口径），由 paths 统一给出
            let plugins_root = paths.plugins_root.clone();
            // 旧版插件在 `<AppData>\plugins`：那里现在仍是配置根下路径
            migrate_plugins_if_needed(&config_dir, &plugins_root);

            // 插件重数据根必须在 load_all 之前设好：加载时会据此注入
            // VA_PLUGIN_DATA_DIR_<ID>，并把旧布局的 plugins/<id>/data 搬过来
            plugins::loader::set_plugin_data_root(&paths.plugin_data_root());

            // 移动端遥控已本体化：在插件加载前清理旧 lan-remote（迁移配对 token +
            // 删注册表条目/插件目录/残留内置 zip），避免其被重新加载与本体服务抢 45271
            // 配对记录属配置 → 走配置根
            remote::purge_legacy_plugin(&plugins_root, &config_dir);

            // 已下架的内置插件（映射表见 plugins::REPLACED_PLUGINS）：bootstrap 只装不删，
            // 老用户本地会残留其目录与注册表条目、与承接者并存。按表在 load_all 前一次性清走并迁移设置。
            plugins::sweep_replaced_plugins(&plugins_root, &config_dir);

            // 已下线的本地引擎（见 plugins::RETIRED_PLUGIN_IDS）：清走插件目录与引擎数据并提示一次。
            if let Some(text) =
                plugins::sweep_retired_plugins(&plugins_root, &paths.plugin_data_root())
            {
                app.state::<commands::storage::MigrationNotice>().set(text);
            }

            // 插件系统：加载已安装插件（单个插件失败只记日志，不影响主流程）；
            // 传入 AppHandle 供宿主能力桥注入（声明 requires_host_bridge 的插件）
            let plugin_manager = plugins::PluginManager::load_all(&plugins_root, Some(app.handle()));
            // 通用插件配置注入：按各插件 manifest 的 config 声明把
            // settings.plugin_config 注入环境变量（替代旧 minimax 硬编码注入）
            plugin_manager.inject_config_env(&settings.plugin_config);
            app.manage(plugin_manager);
            // 补注入 load_all 期间挂起的能力桥（attach 回调需取到已 manage 的 PluginManager）
            app.state::<plugins::PluginManager>().attach_pending_bridges();

            // 远程配置预热：后台拉一次并写本地缓存，供 get_plugin_config 的 help_url
            // 覆盖读取（那条命令是同步的、不能联网）。失败静默，不阻塞启动。
            // ⚠ 必须用配置根：缓存的读侧（get_remote_config / effective_help_url）走的是
            // state.config_dir。之前这里传的是 content_dir（内容根），翻默认前两根相等看不出来，
            // 翻完就变成“写在一个文件、读另一个”——远程 help_url 覆盖会静默失效。
            commands::remote::prime_cache(config_dir.clone());

            // 移动端遥控（本体常驻服务）：WS 服务 + mDNS 广播。须在 HostBridge/AppState/
            // MicPlayback 就绪后启动（native_* 能力依赖三者）；RemoteCore 供 remote_session_info 读取
            // ⚠ 必须传配置根：spawn 内的 dir 只用于 Pairing::load 读 remote-pairing.json（属配置），
            // 翻默认前两根相等看不出来，传内容根会导致配对读写错根（老用户免重配失效）。
            let remote_shared = remote::spawn(app.handle().clone(), config_dir.clone());
            app.manage(remote::RemoteCore::new(remote_shared));

            // 字幕监听运行态（采集/VAD/ASR 编排会话的启停与历史）
            app.manage(commands::subtitle::SubtitleState::new());

            // 应用内升级运行态（下载忙碌位与取消标志）
            app.manage(commands::update::AppUpdaterState::new());

            // 浮窗呼出快捷键：读设置 → 注册（失败只记日志，不影响主功能）
            let accel = settings.hotkey_show_window.clone();
            let register_ok = match hotkey::register_hotkey(app.handle(), &accel) {
                Ok(()) => true,
                Err(e) => {
                    log_error!("注册快捷键 {accel} 失败: {e}");
                    false
                }
            };
            app.manage(HotkeyState {
                current: Mutex::new(register_ok.then_some(accel)),
            });
            app.manage(FavoriteHotkeys::new());

            // 语音输入快捷键（按住说话）：非空才注册，失败只记日志
            let vi_accel = settings.voice_input_hotkey.clone();
            let vi_ok = if vi_accel.is_empty() {
                false
            } else {
                match hotkey::register_voice_input_hotkey(app.handle(), &vi_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        log_error!("注册语音输入快捷键 {vi_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::VoiceInputHotkeyState {
                current: Mutex::new(vi_ok.then_some(vi_accel)),
            });

            // 播放最近一条消息 / 开关麦克风快捷键：非空才注册，失败只记日志
            let pl_accel = settings.hotkey_play_last.clone();
            let pl_ok = if pl_accel.is_empty() {
                false
            } else {
                match hotkey::register_play_last_hotkey(app.handle(), &pl_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        log_error!("注册播放最近消息快捷键 {pl_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::PlayLastHotkeyState {
                current: Mutex::new(pl_ok.then_some(pl_accel)),
            });

            let mt_accel = settings.hotkey_mic_toggle.clone();
            let mt_ok = if mt_accel.is_empty() {
                false
            } else {
                match hotkey::register_mic_toggle_hotkey(app.handle(), &mt_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        log_error!("注册麦克风开关快捷键 {mt_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::MicToggleHotkeyState {
                current: Mutex::new(mt_ok.then_some(mt_accel)),
            });

            // 字幕监听暂停/恢复快捷键（默认 Alt+M）：非空才注册，失败只记日志
            let sp_accel = settings.subtitle_pause_hotkey.clone();
            let sp_ok = if sp_accel.is_empty() {
                false
            } else {
                match hotkey::register_subtitle_pause_hotkey(app.handle(), &sp_accel) {
                    Ok(()) => true,
                    Err(e) => {
                        log_error!("注册字幕监听暂停快捷键 {sp_accel} 失败: {e}");
                        false
                    }
                }
            };
            app.manage(hotkey::SubtitlePauseHotkeyState {
                current: Mutex::new(sp_ok.then_some(sp_accel)),
            });

            // 加载收藏用于注册收藏快捷键（content_dir 随后移入 AppState）
            let favorites = storage::favorites::load_favorites(&content_dir);

            // 焦点诊断：记录 quick_input 焦点变化与当时的前台窗口（排查游戏内浮窗自动消失）
            if let Some(qi) = app.get_webview_window("quick_input") {
                // 直播伴侣形态：浮窗永久挂 WS_EX_NOACTIVATE——点击只送达鼠标消息、
                // 不激活窗口，前台永远是游戏；打字靠 SetFocus webview 子窗口建立键盘路由。
                crate::win32::set_no_activate(&qi, true);
                let log_dir = content_dir.clone(); // content_dir 之后还要移入 AppState，先克隆
                qi.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(focused) = event {
                        let (hwnd, title) = crate::win32::foreground_info();
                        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                        let line = format!("[{ts}] quick_input focused={focused}，当时前台窗口: \"{title}\" (hwnd=0x{hwnd:X})");
                        eprintln!("{line}");
                        use std::io::Write;
                        let path = log_dir.join("focus_debug.log");
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                            let _ = writeln!(f, "{line}");
                        }
                    }
                });
            }

            // AppState / MicPlayback 已在插件加载前 manage（宿主能力桥依赖），此处不再注册

            // 注册收藏快捷键（需在 AppState/MicPlayback manage 之后）
            if let Err(e) = hotkey::refresh_favorite_hotkeys(app.handle(), &favorites) {
                log_error!("注册收藏快捷键失败: {e}");
            }
            // 系统托盘
            tray::setup(app)?;
            tray::install_close_to_tray(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crate::commands::tts::generate_tts,
            crate::commands::tts_stream::generate_tts_stream,
            crate::commands::tts_stream::tts_stream_stop,
            crate::commands::tts_stream::tts_stream_mute,
            crate::commands::perf::perf_startup_done,
            crate::commands::diag::export_diagnostics,
            crate::commands::plugins::list_plugins,
            crate::commands::plugins::uninstall_plugin,
            crate::commands::plugins::get_plugin_config,
            crate::commands::plugins::set_plugin_config,
            crate::commands::plugins::clear_plugin_config,
            crate::commands::plugins::install_plugin_zip,
            crate::commands::plugins::fetch_plugin_index,
            crate::commands::plugins::download_install_plugin,
            crate::commands::plugins::list_bundled_plugins,
            crate::commands::plugins::install_bundled_plugin,
            crate::commands::plugins::run_plugin_setup,
            crate::commands::plugins::install_voice,
            crate::commands::plugins::uninstall_voice,
            crate::commands::plugins::preload_voice,
            crate::commands::plugins::import_voice_pack,
            crate::commands::update::check_app_update,
            crate::commands::update::download_app_update,
            crate::commands::update::cancel_app_update,
            crate::commands::update::install_app_update,
            crate::commands::remote::get_remote_config,
            crate::commands::remote::remote_lan_status,
            crate::commands::remote::remote_firewall_open,
            crate::remote::remote_session_info,
            crate::commands::message::list_messages,
            crate::commands::message::delete_message,
            crate::commands::favorite::list_favorites,
            crate::commands::favorite::add_favorite,
            crate::commands::favorite::delete_favorite,
            crate::commands::favorite::import_favorite,
            crate::commands::favorite::set_favorite_hotkey,
            crate::commands::favorite::remove_favorite_hotkey,
            crate::commands::settings::get_settings,
            crate::commands::settings::update_setting,
            crate::commands::storage::storage_report,
            crate::commands::storage::storage_choice_pending,
            crate::commands::storage::confirm_default_content_root,
            crate::commands::storage::set_content_root,
            crate::commands::storage::take_storage_notice,
            crate::commands::storage::plugin_data_size,
            crate::commands::storage::delete_plugin_data,
            crate::commands::storage::migrate_storage_now,
            crate::commands::storage::restart_app,
            crate::commands::audio::resolve_audio_url,
            crate::commands::clone_voice::import_clone_voice,
            crate::commands::clone_voice::remove_clone_voice,
            crate::commands::mic::list_mic_devices,
            crate::commands::mic::check_vb_cable,
            crate::commands::mic::play_to_mic,
            crate::commands::mic::test_mic,
            crate::commands::mic::stop_mic,
            crate::commands::mic::get_mic_status,
            crate::commands::vbcable::download_vb_cable,
            crate::commands::vbcable::install_vb_cable,
            crate::asr::list_asr_plugins,
            crate::asr::asr_transcribe,
            crate::commands::subtitle::list_audio_processes,
            crate::commands::subtitle::start_audio_listener,
            crate::commands::subtitle::stop_audio_listener,
            crate::commands::subtitle::subtitle_status,
            crate::commands::subtitle::set_subtitle_paused,
            crate::commands::subtitle::get_subtitle_sessions,
            crate::commands::subtitle::export_subtitle_history,
            crate::hotkey::set_subtitle_pause_hotkey,
            crate::commands::minimax_clone::minimax_global_voice_clone,
            crate::commands::minimax_clone::minimax_global_get_voices,
            crate::commands::minimax_clone::minimax_global_delete_voice,
            crate::hotkey::set_hotkey,
            crate::hotkey::set_voice_input_hotkey,
            crate::hotkey::set_play_last_hotkey,
            crate::hotkey::set_mic_toggle_hotkey,
            crate::hotkey::focus_quick_input_content,
            crate::show_main_window,
            crate::window_size::main_window_ready,
        ])
        .run(tauri::generate_context!())
        .expect("VoiceAssist 启动失败");
}
