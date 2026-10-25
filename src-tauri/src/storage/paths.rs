// 存储路径的唯一权威（设计文档 doc/存储布局与迁移设计.md §三契约 1）。
//
// 为什么要单独一个模块：全项目原本只有一处向系统要数据目录（lib.rs 的 app_data_dir），
// 但“数据放在哪”的知识散在 19 个模块的 join("audio") / join("voice_samples") 里。
// 配置与内容分根之后，任何一处自己拼路径都会静默地把数据写错地方 —— 所以禁止，所以在此集中。
//
// 规则（AGENTS.md「存储契约」同步记载，并由 tools/check-storage-hygiene.ps1 机器检查）：
//   1. 业务代码不得调 app_data_dir() / current_exe() 来拼数据路径；
//   2. 记录型文件里不得写绝对路径（audio_path 等一律相对内容根）；
//   3. 新增数据目录先加到本模块，再使用。

// ⚠ 临时豁免（存储计划 Task 3/4 把 getter 全部接上后必须移除）：现在只接线了启动解析，
// 多数 getter 还没有调用方，不加这一条会刷一片 dead_code warning，破坏「cargo check 零 warning」门禁。
// 移除时不得靠提高 allow 范围绕过，必须接完一个删一个。
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// 内容根下的子目录名：**字面量只存在这一份**。
/// 记录里存的就是这些名字开头的相对路径（存储契约：不得写绝对路径），
/// 所以它们既是磁盘布局也是数据格式——改名字等于改数据格式，必须配合迁移。
pub const AUDIO_DIR_NAME: &str = "audio";
pub const SAMPLES_DIR_NAME: &str = "voice_samples";
pub const DOWNLOADS_DIR_NAME: &str = "downloads";
pub const CACHE_DIR_NAME: &str = "cache";
pub const LOGS_DIR_NAME: &str = "logs";
pub const PLUGIN_DATA_DIR_NAME: &str = "plugin-data";

/// 内容根下的记录类文件名：它们与 audio/ 必须同根（删消息是按引用计数删音频的）。
/// 搬家/回迁/备份识别共用这份字面量，不在业务代码里各写一遍。
pub const MESSAGES_FILE_NAME: &str = "messages.json";
pub const FAVORITES_FILE_NAME: &str = "favorites.json";
pub const SUBTITLE_HISTORY_FILE_NAME: &str = "subtitle_history.json";

/// 由内容根取子目录（业务代码只拿到 `&Path` 时用它们，不要自己 join 字面量）
pub fn audio_dir_in(content_root: &Path) -> PathBuf { content_root.join(AUDIO_DIR_NAME) }
pub fn samples_dir_in(content_root: &Path) -> PathBuf { content_root.join(SAMPLES_DIR_NAME) }
pub fn downloads_dir_in(content_root: &Path) -> PathBuf { content_root.join(DOWNLOADS_DIR_NAME) }

/// 记录里用的相对音频路径（存储契约：只存相对内容根的路径）
pub fn audio_rel(id: &str, ext: &str) -> String { format!("{AUDIO_DIR_NAME}/{id}.{ext}") }

/// 记录里用的相对克隆样本路径
pub fn samples_rel(name: &str, ext: &str) -> String { format!("{SAMPLES_DIR_NAME}/{name}.{ext}") }

/// 应用标识符，同时是应用目录名（容器）。主程序走 Tauri 的 `app_data_dir()`，
/// 命令行急救程序没有 AppHandle、只能自己按 `%APPDATA%\<这个名>` 拼，
/// 所以用一条单测把它钉在 tauri.conf.json 上，防止两边拼错到不同目录。
pub const CONFIG_DIR_NAME: &str = "com.voiceassist.app";
/// 内容根指针的文件名（字面量只存这一份，`pointer_file()` 与急救程序都用它）
pub const POINTER_FILE_NAME: &str = "storage.json";
/// 应用目录下的配置子目录名：配置文件全部在这里，应用目录根只做容器
pub const CONFIG_SUBDIR_NAME: &str = "config";
/// 应用目录下的默认「我的数据」子目录名（用户未自选时的落点）
pub const DATA_SUBDIR_NAME: &str = "data";

/// 三根的持有者。由 `lib.rs` 的 `resolve_storage_paths` 在启动时构造一次，之后只读。
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePaths {
    /// 应用目录：`%APPDATA%\com.voiceassist.app` —— 只做容器。
    /// 旧布局的配置与「我的数据」曾直接放在这里（启动时搬进 config\ 与 data\）。
    pub app_root: PathBuf,
    /// 配置根：`<app_root>\config` —— 只放小而敏感的配置文件
    pub config_root: PathBuf,
    /// 内容与重数据的根，默认为 `<app_root>\data`；用户可自选（storage.json 指针）
    pub content_root: PathBuf,
    /// 插件代码根（manifest + dll + registry），跟随安装位置
    pub plugins_root: PathBuf,
    /// 内容根因不可写等原因回退时为 true → 界面必须向用户说明原因
    pub content_fallback: bool,
    /// 1.9.0 测试版把「我的数据」放在 `<exe>\data`。启动时由唯一解析入口填入，
    /// 供自动回迁与存储报告统计；与内容根相同时表示“没有另一处旧数据”。
    pub legacy_exe_data: Option<PathBuf>,
    /// 插件根因安装目录不可写而回退到 `<内容根>\plugins` 时为 true
    pub plugins_fallback: bool,
}

impl StoragePaths {
    /// 测试与非注入场景用的直构器
    pub fn from_dirs(
        app_root: PathBuf,
        config_root: PathBuf,
        content_root: PathBuf,
        plugins_root: PathBuf,
        content_fallback: bool,
    ) -> Self {
        Self {
            app_root,
            config_root,
            content_root,
            plugins_root,
            content_fallback,
            legacy_exe_data: None,
            plugins_fallback: false,
        }
    }

    // ── 配置根下的条目 ────────────────────────
    pub fn settings_file(&self) -> PathBuf { self.config_root.join("settings.json") }
    pub fn pairing_file(&self) -> PathBuf { self.config_root.join("remote-pairing.json") }
    /// 内容根指针（用户改过位置或迁移过才存在）
    pub fn pointer_file(&self) -> PathBuf { self.config_root.join(POINTER_FILE_NAME) }
    /// 迁移中间产物（原件移入、不删除，可回退）。放在应用目录根：它既不是配置也不是
    /// 「我的数据」，且旧版本的备份也在同一层，急救工具扫一处就能找到全部。
    pub fn backup_dir(&self, stamp: &str) -> PathBuf {
        self.app_root.join(format!(".migrated-backup-{stamp}"))
    }

    // ── 内容根下的条目 ────────────────────────
    /// 默认「我的数据」位置：`<app_root>\data`
    pub fn default_content_root(&self) -> PathBuf { self.app_root.join(DATA_SUBDIR_NAME) }
    /// 内容根是否仍是默认位置（用于“用户自选过没有”的判定）
    pub fn content_is_default(&self) -> bool { self.content_root == self.default_content_root() }
    pub fn messages_file(&self) -> PathBuf { self.content_root.join("messages.json") }
    pub fn favorites_file(&self) -> PathBuf { self.content_root.join("favorites.json") }
    pub fn subtitle_history_file(&self) -> PathBuf {
        self.content_root.join("subtitle_history.json")
    }
    pub fn audio_dir(&self) -> PathBuf { audio_dir_in(&self.content_root) }
    pub fn samples_dir(&self) -> PathBuf { samples_dir_in(&self.content_root) }
    pub fn logs_dir(&self) -> PathBuf { self.content_root.join(LOGS_DIR_NAME) }
    pub fn cache_dir(&self) -> PathBuf { self.content_root.join(CACHE_DIR_NAME) }
    pub fn downloads_dir(&self) -> PathBuf { downloads_dir_in(&self.content_root) }
    pub fn plugin_data_root(&self) -> PathBuf { self.content_root.join(PLUGIN_DATA_DIR_NAME) }
    pub fn plugin_data_dir(&self, id: &str) -> PathBuf { plugin_data_dir(&self.content_root, id) }

    /// 建齐布局所需目录（幂等，启动时调用一次）
    pub fn ensure_layout(&self) -> std::io::Result<()> {
        for dir in [
            self.config_root.as_path(),
            self.content_root.as_path(),
            self.audio_dir().as_path(),
            self.samples_dir().as_path(),
            self.logs_dir().as_path(),
            self.cache_dir().as_path(),
            self.plugin_data_root().as_path(),
            self.plugins_root.as_path(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// 决定内容根：环境变量 > 指针文件 > 配置根（新默认）> `<exe>\data`（仅历史兜底）；
/// 被选位置不可写则回退 fallback。
///
/// `<exe>\data` 自 1.9.0 起**不再是默认落点**：数据在安装目录里会随卸载一起被删。
/// 它只在 `default_root` 缺失（非正常启动路径）时兜底，正常启动总会传配置根。
/// `writable` 注入以便单测（真实实现是 `probe_writable`）。
pub fn decide_content_root(
    env_dir: Option<PathBuf>,
    pointer: Option<PathBuf>,
    default_root: Option<PathBuf>,
    exe_default: PathBuf,
    fallback: PathBuf,
    writable: &dyn Fn(&Path) -> bool,
) -> (PathBuf, bool) {
    // 显式指定的位置（env / 指针）不可写时不静默改掉用户的意思，而是回退并标记，
    // 让界面能说出“为什么数据没在你要的位置”
    if let Some(p) = env_dir.filter(|p| !p.as_os_str().is_empty()) {
        return if writable(&p) { (p, false) } else { (fallback, true) };
    }
    if let Some(p) = pointer.filter(|p| !p.as_os_str().is_empty()) {
        return if writable(&p) { (p, false) } else { (fallback, true) };
    }
    if let Some(p) = default_root.filter(|p| !p.as_os_str().is_empty()) {
        return if writable(&p) { (p, false) } else { (fallback, true) };
    }
    if writable(&exe_default) {
        return (exe_default, false);
    }
    (fallback, true)
}

/// 决定插件代码根：`VA_PLUGINS_DIR` > `<exe>\plugins`（可写）> `<content_root>\plugins`。
/// 返回 `(root, used_fallback)`。
///
/// 为什么需要回退：装到 `C:\Program Files\TTSassist` 时普通用户写不了安装目录，
/// 装/更新插件会直接失败。回退到「我的数据」下，数据与插件一起可迁移、可备份。
/// 环境变量是开发/急救逃生口，设了就照用（不因不可写而静默改道）。
pub fn decide_plugins_root(
    env_dir: Option<PathBuf>,
    exe_plugins: PathBuf,
    content_root: PathBuf,
    writable: &dyn Fn(&Path) -> bool,
) -> (PathBuf, bool) {
    if let Some(p) = env_dir.filter(|p| !p.as_os_str().is_empty()) {
        return (p, false);
    }
    if writable(&exe_plugins) {
        return (exe_plugins, false);
    }
    (content_root.join("plugins"), true)
}

/// 由 `%APPDATA%` 拼应用目录（拆成纯函数以便单测，不读环境变量）
pub fn app_root_from(appdata: Option<PathBuf>) -> Option<PathBuf> {
    appdata.map(|a| a.join(CONFIG_DIR_NAME))
}

/// 由 `%APPDATA%` 拼配置根（应用目录下的 config\）
pub fn config_root_from(appdata: Option<PathBuf>) -> Option<PathBuf> {
    app_root_from(appdata).map(|a| a.join(CONFIG_SUBDIR_NAME))
}

/// 急救程序用的应用目录解析。拿不到 `APPDATA` 时返回 None，由调用方说明原因而不是猜一个。
pub fn app_root_from_env() -> Option<PathBuf> {
    app_root_from(std::env::var_os("APPDATA").map(PathBuf::from))
}

/// 急救程序用的配置根解析（config\）
pub fn config_root_from_env() -> Option<PathBuf> {
    config_root_from(std::env::var_os("APPDATA").map(PathBuf::from))
}

/// 旧布局直接放在应用目录根的配置文件名（新布局全部进 config\）。
const CONFIG_FILES: [&str; 4] = [
    "settings.json",
    "remote-pairing.json",
    "remote-config-cache.json",
    POINTER_FILE_NAME,
];

/// 解析并（首次启动时）把旧布局的配置搬进 `config\`，返回本次要用的配置根。
///
/// 安全底线：新位置没有 settings.json 就**绝不擅自切换**——建目录/搬迁失败时
/// 继续用应用目录根（旧布局），保证设置始终读得到；复制失败时原件保留。
pub fn ensure_config_root(app_root: &Path) -> PathBuf {
    let config_dir = app_root.join(CONFIG_SUBDIR_NAME);
    if config_dir.join("settings.json").is_file() {
        return config_dir;
    }
    if std::fs::create_dir_all(&config_dir).is_err() {
        return app_root.to_path_buf();
    }
    // 全新安装：应用目录根没有 settings.json → 直接用新目录
    if !app_root.join("settings.json").is_file() {
        return config_dir;
    }
    for name in CONFIG_FILES {
        let src = app_root.join(name);
        let dst = config_dir.join(name);
        if src.is_file() && !dst.exists() {
            if let Err(e) = relocate(&src, &dst) {
                log_warn!("配置迁移失败（{name}）：{e}");
            }
        }
    }
    if config_dir.join("settings.json").is_file() {
        config_dir
    } else {
        log_warn!("配置未能搬入 config\\，本次继续使用旧配置根：{}", app_root.display());
        app_root.to_path_buf()
    }
}

/// 搬单个配置：同卷 rename；跨卷/被占用时复制（原件保留，旧根从此不再被读）。
fn relocate(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::copy(src, dst).map(|_| ()),
    }
}

/// 读指针文件；不存在 / JSON 坏 / 值为空都返回 None（= 走默认位置，不报错不崩）。
pub fn read_pointer(path: &Path) -> Option<PathBuf> {
    let raw = std::fs::read_to_string(path).ok()?;
    // BOM 容忍：PowerShell 写过带 BOM 的 JSON 会让 serde_json 直接解析失败（已踩过）
    let v: serde_json::Value =
        serde_json::from_str(raw.trim_start_matches('\u{FEFF}')).ok()?;
    v.get("content_root")?
        .as_str()
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
}

/// 指针文件内容（无 BOM 写入由调用方的 atomic 层负责）
pub fn pointer_json(content_root: &Path, migrated_from: Option<&str>) -> String {
    let v = serde_json::json!({
        "content_root": content_root.display().to_string(),
        "migrated_from": migrated_from,
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// 插件重数据目录的唯一算法（结构体方法也转发到这里）。
/// 为何单独提供自由函数：loader / 命令层拿到的常常是 `&Path` 而不是 `StoragePaths`，
/// 没有这个入口它们就会回去 join("data") —— 那正是本模块要禁止的事。
pub fn plugin_data_dir(content_root: &Path, id: &str) -> PathBuf {
    content_root
        .join(PLUGIN_DATA_DIR_NAME)
        .join(plugin_dir_name(id))
}

/// 插件目录名安全化：与 manifest.id 的 `a-zA-Z0-9-_` 规则同口径，其余字符换 `_`。
/// 防的是把 `../evil` 这类拼进路径。
pub fn plugin_dir_name(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// 递归求目录字节数。不跟符号链接（虚拟环境里常见链接，跟进去会重复计数甚至死循环）。
/// 只在按需场合用（卸载弹窗、存储页），别放进启动路径。
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(p) else { return 0 };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            total += dir_size(&entry.path());
        } else if ft.is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 真实可写探测：建目录后写一个临时文件再删掉（只清自己造的文件）。
pub fn probe_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".va-write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    // Task 1 Step 1：先只有测试（实现尚未存在，编译错即红证）
    use super::*;
    use std::path::Path;

    #[test]
    fn decide_content_root_无env无指针时默认等于配置根() {
        let cfg = PathBuf::from(r"C:\Users\u\AppData\Roaming\com.voiceassist.app");
        let exe_default = PathBuf::from(r"C:\exe\data");
        let fallback = PathBuf::from(r"C:\Users\u\AppData\Local\com.voiceassist.app\data");
        let (root, used_fallback) = decide_content_root(None, None, Some(cfg.clone()), exe_default, fallback, &|_| true);
        assert_eq!(root, cfg, "默认落点必须是配置根（%APPDATA%），不再是 <exe>\\data");
        assert!(!used_fallback);
    }

    #[test]
    fn decide_content_root_指针优先于默认() {
        let cfg = PathBuf::from(r"C:\cfg");
        let picked = PathBuf::from(r"D:\VoiceAssistData");
        let (root, _) = decide_content_root(None, Some(picked.clone()), Some(cfg), PathBuf::from(r"C:\exe\data"), PathBuf::from(r"C:\fb"), &|_| true);
        assert_eq!(root, picked, "用户自选位置（指针）最高优先（仅次于 env）");
    }

    #[test]
    fn 内容根优先级_环境变量高于指针高于默认() {
        let (root, fb) = decide_content_root(
            Some(PathBuf::from("E:\\env")),
            Some(PathBuf::from("D:\\pointer")),
            Some(PathBuf::from("C:\\default")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("E:\\env"));
        assert!(!fb, "显式指定不算回退");

        let (root, fb) = decide_content_root(
            None,
            Some(PathBuf::from("D:\\pointer")),
            Some(PathBuf::from("C:\\default")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("D:\\pointer"));
        assert!(!fb);

        let (root, _) = decide_content_root(
            None,
            None,
            Some(PathBuf::from("C:\\default")),
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\fallback"),
            &|_| true,
        );
        assert_eq!(root, PathBuf::from("C:\\default"), "默认 = 配置根，不是安装目录");
    }

    #[test]
    fn 不可写时回退并标记() {
        let (root, fb) = decide_content_root(
            None,
            Some(PathBuf::from("E:\\gone")),
            None,
            PathBuf::from("C:\\exe\\data"),
            PathBuf::from("C:\\Users\\x\\AppData\\Local\\com.voiceassist.app\\data"),
            &|p: &Path| p != Path::new("E:\\gone"),
        );
        assert_eq!(
            root,
            PathBuf::from("C:\\Users\\x\\AppData\\Local\\com.voiceassist.app\\data")
        );
        assert!(fb, "非默认位置失效必须标记，界面才能说明原因");
    }

    #[test]
    fn decide_plugins_root_安装目录不可写时回退到我的数据() {
        let exe_plugins = PathBuf::from(r"C:\Program Files\TTSassist\plugins");
        let content = PathBuf::from(r"D:\VoiceAssistData");
        let (root, fallback) = decide_plugins_root(
            None,
            exe_plugins.clone(),
            content.clone(),
            &|p: &Path| p != exe_plugins.as_path(),
        );
        assert_eq!(
            root,
            content.join("plugins"),
            "装到 Program Files 时插件目录必须回退，否则装/更新插件会失败"
        );
        assert!(fallback, "回退必须标记，设置页要说明插件装在哪");
    }

    #[test]
    fn decide_plugins_root_可写时跟随安装目录且环境变量优先() {
        let exe_plugins = PathBuf::from(r"D:\Apps\TTSassist\plugins");
        let content = PathBuf::from(r"C:\Users\u\AppData\Roaming\com.voiceassist.app");
        let (root, fallback) =
            decide_plugins_root(None, exe_plugins.clone(), content, &|_| true);
        assert_eq!(root, exe_plugins, "可写时跟随安装位置（阶段 22 口径）");
        assert!(!fallback);

        let env = PathBuf::from(r"E:\dev-plugins");
        let (root, fallback) = decide_plugins_root(
            Some(env.clone()),
            PathBuf::from(r"Z:\gone"),
            PathBuf::from(r"Z:\data"),
            &|_| false,
        );
        assert_eq!(root, env, "开发/急救用的环境变量优先级最高");
        assert!(!fallback);
    }

    #[test]
    fn 插件目录名安全化_与manifest_id规则同口径() {
        // 用中立名 demo-tts：路径消毒测试不需要真实引擎名（基座护栏也不允许给基座新增命中）
        assert_eq!(plugin_dir_name("demo-tts"), "demo-tts");
        // 空格、斜杠、两个汉字 = 尾部 3 个下划线（中间那个是空格换来的）
        assert_eq!(plugin_dir_name("Demo TTS/中文"), "Demo_TTS___");
        assert!(!plugin_dir_name("..\\evil").contains('\\'));
        assert!(!plugin_dir_name("../evil").contains('/'));
    }

    #[test]
    fn 各归属路径不共用根() {
        let p = StoragePaths::from_dirs(
            PathBuf::from("C:\\app"),
            PathBuf::from("C:\\app\\config"),
            PathBuf::from("D:\\app\\data"),
            PathBuf::from("D:\\app\\plugins"),
            false,
        );
        assert_eq!(p.settings_file(), PathBuf::from("C:\\app\\config\\settings.json"));
        assert_eq!(p.pointer_file(), PathBuf::from("C:\\app\\config\\storage.json"));
        assert_eq!(p.messages_file(), PathBuf::from("D:\\app\\data\\messages.json"));
        assert_eq!(p.audio_dir(), PathBuf::from("D:\\app\\data\\audio"));
        assert_eq!(
            p.plugin_data_dir("demo-tts"),
            PathBuf::from("D:\\app\\data\\plugin-data\\demo-tts")
        );
        assert_eq!(
            p.backup_dir("20261011"),
            PathBuf::from("C:\\app\\.migrated-backup-20261011"),
            "备份既不属于配置也不属于我的数据，放在应用目录根"
        );
        assert_eq!(p.default_content_root(), PathBuf::from("C:\\app\\data"));
    }

    #[test]
    fn 布局建齐后各目录都存在() {
        let dir = tempfile::tempdir().unwrap();
        let p = StoragePaths::from_dirs(
            dir.path().join("cfg"),
            dir.path().join("cfg").join(CONFIG_SUBDIR_NAME),
            dir.path().join("data"),
            dir.path().join("plugins"),
            false,
        );
        p.ensure_layout().unwrap();
        for d in [
            p.config_root.clone(),
            p.content_root.clone(),
            p.audio_dir(),
            p.samples_dir(),
            p.logs_dir(),
            p.cache_dir(),
            p.plugin_data_root(),
            p.plugins_root.clone(),
        ] {
            assert!(d.is_dir(), "缺少目录 {}", d.display());
        }
    }

    #[test]
    fn 相对路径格式与子目录名单一来源() {
        // 这两个格式就是写进 messages.json / settings.json 的值，
        // 改它们等于改数据格式（必须配合迁移），所以用测钉住
        assert_eq!(audio_rel("m_1", "mp3"), "audio/m_1.mp3");
        assert_eq!(samples_rel("clone", "wav"), "voice_samples/clone.wav");
        assert_eq!(
            audio_dir_in(Path::new("D:\\app\\data")),
            PathBuf::from("D:\\app\\data\\audio")
        );
    }

    #[test]
    fn 指针文件坏内容当没有() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("storage.json");
        assert!(read_pointer(&p).is_none(), "不存在 → None");
        std::fs::write(&p, "not json").unwrap();
        assert!(read_pointer(&p).is_none(), "格式坏 → None，不能崩");
        std::fs::write(&p, "{\"content_root\":\"  \"}").unwrap();
        assert!(read_pointer(&p).is_none(), "空值 → None，走默认位置");
        std::fs::write(&p, "{\"content_root\":\"D:\\\\VA\\\\data\"}").unwrap();
        assert_eq!(read_pointer(&p), Some(PathBuf::from("D:\\VA\\data")));
    }

    #[test]
    fn 配置根拼接与标识符对齐配置文件() {
        // 急救程序自己拼应用目录，拼错就等于“找不到用户的备份”。
        // 不靠人记，直接拿 tauri.conf.json 的 identifier 对账。
        assert_eq!(
            app_root_from(Some(PathBuf::from("C:\\Users\\x\\AppData\\Roaming"))),
            Some(PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\com.voiceassist.app"))
        );
        assert_eq!(
            config_root_from(Some(PathBuf::from("C:\\Users\\x\\AppData\\Roaming"))),
            Some(PathBuf::from("C:\\Users\\x\\AppData\\Roaming\\com.voiceassist.app\\config")),
            "配置根是应用目录下的 config\\"
        );
        assert!(app_root_from(None).is_none(), "拿不到 APPDATA 就给 None，不猜路径");

        let conf = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
            .expect("读不到 tauri.conf.json");
        let v: serde_json::Value =
            serde_json::from_str(conf.trim_start_matches('\u{FEFF}')).expect("tauri.conf.json 不是合法 JSON");
        assert_eq!(
            v["identifier"].as_str(),
            Some(CONFIG_DIR_NAME),
            "应用目录名与 tauri.conf.json 的 identifier 不一致，急救程序会找错地方"
        );
    }

    #[test]
    fn 旧布局配置搬进config子目录且幂等() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        std::fs::write(app.join("settings.json"), b"{\"a\":1}").unwrap();
        std::fs::write(app.join("storage.json"), b"{}").unwrap();
        std::fs::write(app.join("messages.json"), b"[]").unwrap();

        let cfg = ensure_config_root(app);

        assert_eq!(cfg, app.join(CONFIG_SUBDIR_NAME));
        assert!(cfg.join("settings.json").is_file(), "配置必须进 config\\");
        assert!(cfg.join("storage.json").is_file());
        assert!(!cfg.join("messages.json").exists(), "内容不能被当成配置搬走");
        assert!(app.join("messages.json").exists(), "内容留在应用目录根，交给内容迁移");
        // 幂等：第二次直接命中
        assert_eq!(ensure_config_root(app), cfg);
    }

    #[test]
    fn 全新安装直接使用config子目录() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ensure_config_root(dir.path());
        assert_eq!(cfg, dir.path().join(CONFIG_SUBDIR_NAME));
        assert!(cfg.is_dir());
    }

    #[test]
    fn 旧布局升级到config与data完整走通() {
        use crate::storage::migrate::{detect_in, migrate_items, StorageState};
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        // 旧布局：配置与「我的数据」都直接放在应用目录根
        std::fs::write(app.join("settings.json"), b"{\"theme\":\"dark\"}").unwrap();
        std::fs::write(app.join(POINTER_FILE_NAME), pointer_json(app, None)).unwrap();
        std::fs::write(app.join("messages.json"), b"[]").unwrap();
        std::fs::create_dir_all(app.join(AUDIO_DIR_NAME)).unwrap();
        std::fs::write(app.join(AUDIO_DIR_NAME).join("m.wav"), b"x").unwrap();
        std::fs::create_dir_all(app.join(PLUGIN_DATA_DIR_NAME).join("demo-tts")).unwrap();
        std::fs::write(
            app.join(PLUGIN_DATA_DIR_NAME).join("demo-tts").join("model.bin"),
            b"m",
        )
        .unwrap();

        // 1) 配置搬进 config\
        let cfg = ensure_config_root(app);
        assert_eq!(cfg, app.join(CONFIG_SUBDIR_NAME));
        assert!(cfg.join("settings.json").is_file());
        assert!(!app.join("settings.json").exists(), "配置应从旧根移走");

        // 2) 指针指向旧默认 → 视为未自选，内容根用 data\
        assert_eq!(
            read_pointer(&cfg.join(POINTER_FILE_NAME)).as_deref(),
            Some(app),
            "旧指针被搬进了 config\\"
        );
        let content = app.join(DATA_SUBDIR_NAME);
        let paths = StoragePaths::from_dirs(
            app.to_path_buf(),
            cfg.clone(),
            content.clone(),
            dir.path().join("plugins"),
            false,
        );
        assert!(paths.content_is_default());
        assert_eq!(detect_in(&paths), StorageState::Migratable);

        // 3) 自动迁移：内容进 data\，引擎数据跟上，原件进备份
        let report = migrate_items(app, &content, "20261023").unwrap();
        assert!(content.join("messages.json").is_file());
        assert!(content.join(AUDIO_DIR_NAME).join("m.wav").is_file());
        assert!(
            content.join(PLUGIN_DATA_DIR_NAME).join("demo-tts").join("model.bin").is_file(),
            "引擎数据必须跟着走，否则升级后要重新下载模型"
        );
        assert!(!app.join("messages.json").exists(), "旧根内容应清空");
        assert!(report.backup.join("messages.json").is_file(), "原件进备份可回退");
        assert_eq!(detect_in(&paths), StorageState::Current);
    }
}
