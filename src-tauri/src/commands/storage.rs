// 「存储」页与卸载弹窗的后端读口：谁占了多大、哪些是残留、能不能搬家。
//
// 设计文档 §六：体积统计是递归求和，大目录可达数万文件，因此三条命令一律走
// blocking 线程，不占 Tauri 主 runtime；扫描失败宁可报空也不要让整个页面打不开。
//
// 路径一律向 `StoragePaths` 要，不在这里拼字面量（AGENTS.md 存储契约）。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::AppState;
use crate::plugins::PluginManager;
use crate::storage::migrate::{self, StorageState};
use crate::storage::paths::{self, StoragePaths};

/// 一类占用。`kind` 是给前端判断用的稳定标识，`label` 给人看
#[derive(Serialize)]
pub struct StorageBucket {
    pub kind: String,
    pub label: String,
    pub path: String,
    pub bytes: u64,
}

/// plugin-data 里的残留：数据还在、插件已不在注册表（设计 §六.2）
#[derive(Serialize)]
pub struct OrphanData {
    pub id: String,
    pub path: String,
    pub bytes: u64,
}

/// 一块磁盘的可用/总容量，首启弹窗与存储页据此提示“放哪个盘”
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct DriveInfo {
    /// 盘符，如 "C:"
    pub letter: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Serialize)]
pub struct StorageReport {
    pub config_root: String,
    pub content_root: String,
    /// true 时界面必须说明为什么数据没落在安装目录（设计 §五：绝不静默两处混写）
    pub content_fallback: bool,
    pub buckets: Vec<StorageBucket>,
    pub orphans: Vec<OrphanData>,
    /// 见 `migration_tag`：只有 "none" / "migratable" / "conflict" 三种取值
    pub migration: String,
    /// 旧位置待迁移项的总体积，界面拿它写「要搬 3.2 GB」
    pub legacy_bytes: u64,
    /// 用户是否已确认过「我的数据」位置（false = 首启弹窗仍应出现）
    pub storage_choice_made: bool,
    /// 1.9.0 测试版留在 `<exe>\data` 的数据体积（0 = 没有另一处旧数据）
    pub legacy_exe_data_bytes: u64,
    /// 本机磁盘剩余空间，首启弹窗展示
    pub drives: Vec<DriveInfo>,
    /// 插件代码根（manifest + dll），用于界面上说明插件装在哪
    pub plugins_root: String,
    /// true = 安装目录不可写，插件根已回退到 `<我的数据>\plugins`
    pub plugins_fallback: bool,
}

/// 「更改数据位置」的执行结果（前端据此提示搬了多少、原件在哪、要不要重启）
#[derive(Serialize, Debug, Default, Clone)]
pub struct StorageMoveReport {
    pub moved_bytes: u64,
    pub moved_files: usize,
    /// 目标位置已有同名而跳过的项（绝不覆盖）
    pub skipped: Vec<String>,
    pub backup_dir: String,
    pub need_restart: bool,
}

/// 迁移状态对外只暴露三种标签（前端按它决定按钮文案，取值不得漂移）
pub fn migration_tag(state: StorageState) -> &'static str {
    match state {
        StorageState::Current | StorageState::Fresh => "none",
        StorageState::Migratable => "migratable",
        StorageState::Conflict => "conflict",
    }
}

/// 插件 id 规则与 manifest.id 同口径：字母/数字/-/_，长度 1~64。
/// `plugin_data_dir` 内部已会把非法字符换成 `_`，这里再拦一道是要**报错**，
/// 而不是静默地把 `../evil` 变成某个合法目录然后删掉它。
pub fn check_plugin_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err(format!("插件 id 长度 {} 不合法（应为 1~64）", id.len()));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("插件 id「{id}」含非法字符（只允许字母、数字、-、_）"));
    }
    Ok(())
}

/// 只读枚举 plugin-data 里「插件已不在注册表」的残留目录，按名字典序稳定输出。
/// 目录不存在当空处理：从没装过带重数据的插件是常态，不该报错。
pub fn orphan_plugin_data(plugin_data_root: &Path, installed: &[String]) -> Result<Vec<String>, String> {
    let Ok(entries) = std::fs::read_dir(plugin_data_root) else {
        return Ok(Vec::new());
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // 只认目录：散文件与 . 开头的隐藏/临时项不是引擎数据，
            // 认错了就会把用户「可一键删除的数据」列表变成删备份的入口
            if !e.path().is_dir() || name.starts_with('.') {
                return None;
            }
            // 比较走同一个安全化函数：注册表 id 与磁盘目录名理论上可能差在非法字符上
            if installed.iter().any(|id| paths::plugin_dir_name(id) == name) {
                None
            } else {
                Some(name)
            }
        })
        .collect();
    out.sort();
    Ok(out)
}

/// 汇总存储报告。纯函数（状态都从参数进来），便于单测与在 blocking 线程里跑。
/// 磁盘容量来自系统查询，单测走 `build_report_with_drives` 注入。
pub fn build_report(paths: &StoragePaths, installed: &[String], storage_choice_made: bool) -> StorageReport {
    build_report_with_drives(paths, installed, storage_choice_made, enumerate_drives())
}

fn build_report_with_drives(
    paths: &StoragePaths,
    installed: &[String],
    storage_choice_made: bool,
    drives: Vec<DriveInfo>,
) -> StorageReport {
    let mut buckets = vec![
        bucket("records", "消息与收藏", &paths.content_root, records_bytes(paths)),
        bucket("audio", "合成音频", &paths.audio_dir(), paths::dir_size(&paths.audio_dir())),
        bucket(
            "samples",
            "克隆样本",
            &paths.samples_dir(),
            paths::dir_size(&paths.samples_dir()),
        ),
        bucket("logs", "日志", &paths.logs_dir(), paths::dir_size(&paths.logs_dir())),
        bucket("cache", "缓存", &paths.cache_dir(), paths::dir_size(&paths.cache_dir())),
        bucket(
            "downloads",
            "下载内容",
            &paths.downloads_dir(),
            paths::dir_size(&paths.downloads_dir()),
        ),
        bucket(
            "plugin_data",
            "引擎数据",
            &paths.plugin_data_root(),
            paths::dir_size(&paths.plugin_data_root()),
        ),
    ];
    buckets.retain(|b| b.bytes > 0); // 空目录不占界面行，用户只想知道东西在哪

    let orphans: Vec<OrphanData> = orphan_plugin_data(&paths.plugin_data_root(), installed)
        .unwrap_or_default()
        .into_iter()
        .map(|name| {
            // name 来自上面对本根的枚举，join 它不违反「路径来自 paths」的契约
            let dir = paths.plugin_data_root().join(&name);
            OrphanData {
                id: name,
                path: dir.display().to_string(),
                bytes: paths::dir_size(&dir),
            }
        })
        .collect();

    let state = migrate::detect_in(paths);
    let legacy_bytes = migrate::legacy_items(&paths.config_root)
        .iter()
        .map(|p| migrate::path_size(p))
        .sum();

    StorageReport {
        config_root: paths.config_root.display().to_string(),
        content_root: paths.content_root.display().to_string(),
        content_fallback: paths.content_fallback,
        buckets,
        orphans,
        migration: migration_tag(state).to_string(),
        legacy_bytes,
        storage_choice_made,
        legacy_exe_data_bytes: legacy_exe_data_bytes(paths),
        drives,
        plugins_root: paths.plugins_root.display().to_string(),
        plugins_fallback: paths.plugins_fallback,
    }
}

/// `<exe>\data` 里的旧数据体积。仅当它确实是**另一处**位置时才算：
/// 内容根当前还默认跟随安装目录时两者相同，不能提示用户“有旧数据待迁”。
fn legacy_exe_data_bytes(paths: &StoragePaths) -> u64 {
    paths
        .legacy_exe_data
        .as_ref()
        .filter(|p| **p != paths.content_root)
        .map(|p| migrate::path_size(p))
        .unwrap_or(0)
}

/// 枚举本机磁盘及其剩余空间（首启弹窗与存储页用）。
/// 取不到容量的盘（光驱无盘等）直接跳过，不让界面出现 0/0 的怪条目。
#[cfg(windows)]
fn enumerate_drives() -> Vec<DriveInfo> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDriveStringsW};

    let len = unsafe { GetLogicalDriveStringsW(None) };
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let written = unsafe { GetLogicalDriveStringsW(Some(&mut buf[..])) };
    if written == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for root in buf[..written as usize].split(|c| *c == 0) {
        if root.is_empty() {
            continue;
        }
        let mut wide: Vec<u16> = root.to_vec();
        wide.push(0);
        let (mut free, mut total) = (0u64, 0u64);
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                PCWSTR(wide.as_ptr()),
                Some(&mut free as *mut u64),
                Some(&mut total as *mut u64),
                None,
            )
            .is_ok()
        };
        if !ok {
            continue;
        }
        let letter = String::from_utf16_lossy(root)
            .trim_end_matches('\\')
            .to_string();
        out.push(DriveInfo { letter, free_bytes: free, total_bytes: total });
    }
    out.sort_by(|a, b| a.letter.cmp(&b.letter));
    out
}

#[cfg(not(windows))]
fn enumerate_drives() -> Vec<DriveInfo> {
    Vec::new()
}

/// 三个记录类 JSON 的合计体积（它们是内容根的直接子文件，不在任何子目录里）
fn records_bytes(paths: &StoragePaths) -> u64 {
    [
        paths.messages_file(),
        paths.favorites_file(),
        paths.subtitle_history_file(),
    ]
    .iter()
    .map(|f| migrate::path_size(f))
    .sum()
}

fn bucket(kind: &str, label: &str, path: &Path, bytes: u64) -> StorageBucket {
    StorageBucket {
        kind: kind.to_string(),
        label: label.to_string(),
        path: path.display().to_string(),
        bytes,
    }
}

/// 某插件的重数据体积（目录不存在给 0）。
/// 单独提供而不放进 `PluginManager::list()`：递归求和可能扫到数万文件，
/// 插件页一进就扫全部插件会把主 runtime 拖住（设计 §六.4：体积统计要按需跑）。
pub fn plugin_data_bytes(paths: &StoragePaths, id: &str) -> u64 {
    paths::dir_size(&paths.plugin_data_dir(id))
}

/// 卸载弹窗按需求体积：装着的插件**不会出现在 orphans 里**（残留的定义就是未安装），
/// 所以这个数只能单独查。
#[tauri::command]
pub async fn plugin_data_size(id: String, app: AppHandle) -> Result<u64, String> {
    check_plugin_id(&id)?;
    let paths = app.state::<StoragePaths>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || plugin_data_bytes(&paths, &id))
        .await
        .map_err(|e| format!("体积扫描任务失败: {e}"))
}

/// 首启自动迁移的一次性提示。
///
/// 为什么不用 event：搬迁在 setup 里完成，那时前端的 listen 还没注册，emit 出去必丢；
/// 而启动时拉一次命令是一定能拿到的。取走即清空，不会下次启动再弹一遍。
#[derive(Default)]
pub struct MigrationNotice(pub std::sync::Mutex<Option<String>>);

impl MigrationNotice {
    pub fn set(&self, text: String) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(text);
        }
    }
}

/// 前端启动时取一次搬家结果（没有则 None）。同步命令：只读一个内存位。
#[tauri::command]
pub fn take_storage_notice(state: State<'_, MigrationNotice>) -> Option<String> {
    state.0.lock().ok().and_then(|mut guard| guard.take())
}

/// 存储页打开时读一次：两根位置、各类占用、残留清单、能否搬家。
#[tauri::command]
pub async fn storage_report(app: AppHandle) -> Result<StorageReport, String> {
    let paths = app.state::<StoragePaths>().inner().clone();
    let installed: Vec<String> = app
        .state::<PluginManager>()
        .list()
        .iter()
        .map(|p| p.id.clone())
        .collect();
    let choice_made = app
        .state::<AppState>()
        .settings
        .read()
        .map(|s| s.storage_choice_made)
        .map_err(|e| format!("读取设置失败: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || build_report(&paths, &installed, choice_made))
        .await
        .map_err(|e| format!("存储扫描任务失败: {e}"))
}

/// 首启弹窗是否还需要出现：用户没确认过位置就为 true。
/// 老用户升级时字段缺失 → false（未选）→ 弹一次，之后不再打扰。
#[tauri::command]
pub fn storage_choice_pending(state: State<'_, AppState>) -> Result<bool, String> {
    state
        .settings
        .read()
        .map(|s| !s.storage_choice_made)
        .map_err(|e| format!("读取设置失败: {e}"))
}

/// 用户确认「就用默认位置」：记下选择，首启弹窗不再出现。
#[tauri::command]
pub fn confirm_default_content_root(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    mark_storage_choice_made(&app, &state)
}

/// 把「已选择数据位置」落盘并同步内存（两条确认路径共用）。
fn mark_storage_choice_made(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let updated = crate::storage::settings::update_setting(
        &state.config_dir,
        "storage_choice_made",
        serde_json::json!(true),
    )
    .map_err(|e| format!("保存设置失败: {e}"))?;
    let mut guard = state
        .settings
        .write()
        .map_err(|e| format!("更新内存设置失败: {e}"))?;
    *guard = updated;
    crate::sync::notify_changed(app, crate::sync::EVENT_SETTINGS_CHANGED);
    Ok(())
}

// ── 搬家（更改「我的数据」位置）──────────────────────────────

/// 待搬的一项：`rel` 是相对内容根的 `/` 分隔路径（同时也是备份里的归档结构）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveItem {
    pub rel: String,
    pub bytes: u64,
}

/// 搬家计划：只读扫描一次，决定搬什么、跳过什么
#[derive(Debug, Default)]
pub struct MovePlan {
    pub items: Vec<MoveItem>,
    /// 目标已有同名的项：**绝不覆盖**，原件留在旧位置
    pub skipped: Vec<String>,
    pub total_bytes: u64,
}

/// 搬家的内容类清单。与 migrate 的 CONTENT_* 同口径，但**不含** cache/downloads/logs：
/// 那是可重建的临时文件，搬过去只会拖慢第一次启动（设计 D7）。
const MOVE_FILES: [&str; 3] = [
    paths::MESSAGES_FILE_NAME,
    paths::FAVORITES_FILE_NAME,
    paths::SUBTITLE_HISTORY_FILE_NAME,
];
const MOVE_DIRS: [&str; 3] = [
    paths::AUDIO_DIR_NAME,
    paths::SAMPLES_DIR_NAME,
    paths::PLUGIN_DATA_DIR_NAME,
];

fn join_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, seg| p.join(seg))
}

fn collect_rel_files(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        if ft.is_dir() {
            collect_rel_files(&entry.path(), &rel, out);
        } else if ft.is_file() {
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((rel, bytes));
        }
    }
}

/// 扫描内容根，生成搬家计划（不写任何东西）。
/// 目标已有同名就跳过：用户的旧数据只有一份，覆盖等于销毁。
pub fn build_move_plan(src: &Path, dst: &Path) -> MovePlan {
    let mut found: Vec<(String, u64)> = Vec::new();
    for name in MOVE_FILES {
        let p = src.join(name);
        if p.is_file() {
            let bytes = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            found.push((name.to_string(), bytes));
        }
    }
    for name in MOVE_DIRS {
        let p = src.join(name);
        if p.is_dir() {
            collect_rel_files(&p, name, &mut found);
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));

    let mut plan = MovePlan::default();
    for (rel, bytes) in found {
        if join_rel(dst, &rel).exists() {
            plan.skipped.push(rel);
        } else {
            plan.total_bytes += bytes;
            plan.items.push(MoveItem { rel, bytes });
        }
    }
    plan
}

#[derive(Debug, Default)]
struct CopyOutcome {
    copied: Vec<String>,
    /// 扫描后、复制前目标被写入的项（同样不覆盖）
    skipped: Vec<String>,
    bytes: u64,
}

/// 逐项复制并校验字节数。任一项失败 → 删掉本次已复制的目标再报错，**源侧一律不动**。
fn copy_content_verified(src: &Path, dst: &Path, plan: &MovePlan) -> Result<CopyOutcome, String> {
    let mut out = CopyOutcome::default();
    for item in &plan.items {
        let from = join_rel(src, &item.rel);
        let to = join_rel(dst, &item.rel);
        if to.exists() {
            out.skipped.push(item.rel.clone());
            continue;
        }
        if let Some(parent) = to.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                rollback_copied(dst, &out.copied);
                return Err(format!("创建目标目录失败: {e}"));
            }
        }
        // 校验基准取「复制前一刻」的源大小：搬家期间应用还在写旧位置，
        // 用扫描时的体积会因为用户发了一条消息而误判校验失败。
        let expected = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
        if let Err(e) = std::fs::copy(&from, &to) {
            rollback_copied(dst, &out.copied);
            return Err(format!("复制「{}」失败: {e}", item.rel));
        }
        let actual = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
        if actual != expected {
            let _ = std::fs::remove_file(&to);
            rollback_copied(dst, &out.copied);
            return Err(format!("复制校验失败，已回退：{}", item.rel));
        }
        out.bytes += actual;
        out.copied.push(item.rel.clone());
    }
    Ok(out)
}

fn rollback_copied(dst: &Path, rels: &[String]) {
    for rel in rels.iter().rev() {
        let _ = std::fs::remove_file(join_rel(dst, rel));
    }
}

/// 复制全部成功后才把原件移进备份。失败只记日志：数据已经在新位置了，
/// 反过来报错只会让用户以为搬家失败而重复操作。
fn archive_sources(src: &Path, backup: &Path, rels: &[String]) {
    for rel in rels {
        let from = join_rel(src, rel);
        let to = join_rel(backup, rel);
        let Some(parent) = to.parent() else { continue };
        if let Err(e) = std::fs::create_dir_all(parent) {
            log_warn!("搬家原件入备份失败（数据已在新位置，原件保留）: {e}");
            continue;
        }
        let name = to
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // 同日多次搬家时备份里可能已有同名项：改名存放，绝不覆盖旧归档
        let target = migrate::free_backup_target(parent, &name);
        if let Err(e) = move_file(&from, &target) {
            log_warn!("搬家原件入备份失败（数据已在新位置，原件保留）: {e}");
        }
    }
}

/// 优先 rename；跨卷失败时退回复制 + 校验 + 删源（与 migrate 的归档同语义）
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let expected = std::fs::metadata(from)?.len();
    std::fs::copy(from, to)?;
    let actual = std::fs::metadata(to)?.len();
    if actual != expected {
        let _ = std::fs::remove_file(to);
        return Err(std::io::Error::other("跨卷复制校验失败"));
    }
    std::fs::remove_file(from)
}

/// 搬家的核心：复制 →（落指针）→ 原件归档。
/// `after_copy` 在复制全部校验通过后、动原件之前执行（生产路径用它写 `storage.json`）。
fn move_content_with(
    src: &Path,
    dst: &Path,
    backup: &Path,
    after_copy: impl FnOnce() -> Result<(), String>,
) -> Result<StorageMoveReport, String> {
    let plan = build_move_plan(src, dst);
    let outcome = copy_content_verified(src, dst, &plan)?;
    after_copy()?;
    archive_sources(src, backup, &outcome.copied);

    let mut skipped = plan.skipped;
    skipped.extend(outcome.skipped);
    Ok(StorageMoveReport {
        moved_bytes: outcome.bytes,
        moved_files: outcome.copied.len(),
        skipped,
        backup_dir: backup.display().to_string(),
        need_restart: true,
    })
}

/// 测试便捷入口（生产路径要写指针，用 `move_content_with`）
#[cfg(test)]
fn move_content(src: &Path, dst: &Path, backup: &Path) -> Result<StorageMoveReport, String> {
    move_content_with(src, dst, backup, || Ok(()))
}

/// 1.9.0 测试版把「我的数据」放在 `<exe>\data`。回迁前提：
/// `<exe>\data\messages.json` 存在，且默认内容根还没有 messages.json。
/// 第三个条件（用户没自选位置）由调用方 `pull_back_legacy_if_needed` 把关。
pub fn should_pull_back_legacy(exe_data: &Path, content_root: &Path) -> bool {
    exe_data.join(paths::MESSAGES_FILE_NAME).is_file()
        && !content_root.join(paths::MESSAGES_FILE_NAME).exists()
}

/// 启动时回迁测试版遗留数据，返回给用户看的一句话（无需回迁则 None）。
///
/// 用户自选过位置（content_root != config_root）时绝不回迁：数据不能悄悄搬到他没选的地方，
/// 否则 D 盘用户升级后会看到另一份空列表。失败只记日志，绝不影响启动。
pub fn pull_back_legacy_if_needed(paths: &StoragePaths) -> Option<String> {
    if paths.content_root != paths.config_root {
        return None;
    }
    let exe_data = paths.legacy_exe_data.as_deref()?;
    if exe_data == paths.content_root || !should_pull_back_legacy(exe_data, &paths.content_root) {
        return None;
    }

    let stamp = chrono::Utc::now().format("%Y%m%d").to_string();
    let backup = paths.backup_dir(&stamp);
    // 回迁不写指针：默认位置就是配置根，指针只在用户自选后才需要
    match move_content_with(exe_data, &paths.content_root, &backup, || Ok(())) {
        Ok(r) if r.moved_files > 0 => {
            let mb = r.moved_bytes as f64 / 1024.0 / 1024.0;
            let text = format!(
                "已把 1.9.0 测试版放在安装目录的消息与音频迁回默认位置（{} 个文件 / {:.1} MB），原件保留在 {}",
                r.moved_files, mb, r.backup_dir
            );
            log_info!("{text}");
            Some(text)
        }
        Ok(_) => None,
        Err(e) => {
            log_warn!("测试版数据自动回迁失败（数据仍在安装目录，可手动拷贝到默认位置）：{e}");
            None
        }
    }
}

/// 更改「我的数据」存放位置：可写校验 → 复制校验 → 写指针 → 旧处转备份。
///
/// 两次搬家（尤其同一天）会复用同一个 `.migrated-backup-<stamp>` 目录，靠
/// `free_backup_target` 给撞名附件改名；被跳过的项留在旧位置，绝不覆盖。
#[tauri::command]
pub async fn set_content_root(path: String, app: AppHandle) -> Result<StorageMoveReport, String> {
    let target = PathBuf::from(path.trim());
    if target.as_os_str().is_empty() {
        return Err("请选择要存放「我的数据」的文件夹".to_string());
    }
    let paths = app.state::<StoragePaths>().inner().clone();
    let src = paths.content_root.clone();
    let pointer_file = paths.pointer_file();
    let backup_root = paths.backup_dir(&chrono::Utc::now().format("%Y%m%d").to_string());

    let report = tauri::async_runtime::spawn_blocking(move || {
        if !paths::probe_writable(&target) {
            return Err(format!("目标位置不可写，请换一个文件夹：{}", target.display()));
        }
        // probe 会建目录，这时 canonicalize 才拿得到真实路径
        let canon_src = std::fs::canonicalize(&src).unwrap_or_else(|_| src.clone());
        let canon_dst = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
        if canon_src == canon_dst {
            // 选的就是当前位置：只记下选择，不搬任何东西
            return Ok(StorageMoveReport { need_restart: false, ..Default::default() });
        }
        if canon_dst.starts_with(&canon_src) || canon_src.starts_with(&canon_dst) {
            return Err("新位置不能与当前位置互相包含，请选择另一个文件夹".to_string());
        }
        move_content_with(&src, &target, &backup_root, || {
            let pointer = paths::pointer_json(&target, Some(&src.display().to_string()));
            crate::storage::atomic::atomic_write(&pointer_file, pointer.as_bytes())
                .map_err(|e| format!("写存储指针失败（数据已复制到新位置，可重试）: {e}"))
        })
    })
    .await
    .map_err(|e| format!("搬家任务失败: {e}"))??;

    let state = app.state::<AppState>();
    mark_storage_choice_made(&app, state.inner())?;
    let mb = report.moved_bytes as f64 / 1024.0 / 1024.0;
    log_info!(
        "「我的数据」位置更新：{} 个文件 / {:.1} MB，原件在 {}",
        report.moved_files,
        mb,
        report.backup_dir
    );
    Ok(report)
}

/// 删除某个「插件已不在」的残留引擎数据目录，返回释放的字节数。
/// 仍安装的插件一律拒绝：数据只能随卸载弹窗一起处置（设计 §六.1，默认保留）。
#[tauri::command]
pub async fn delete_plugin_data(id: String, app: AppHandle) -> Result<u64, String> {
    check_plugin_id(&id)?;
    if app.state::<PluginManager>().is_installed(&id) {
        return Err(format!("插件「{id}」仍在安装列表中，请先卸载插件（卸载时会问要不要一并删除引擎数据）"));
    }
    let dir = app.state::<StoragePaths>().inner().clone().plugin_data_dir(&id);
    tauri::async_runtime::spawn_blocking(move || {
        if !dir.exists() {
            return Ok(0); // 已经被清掉了，重复点击不该报错
        }
        let bytes = paths::dir_size(&dir);
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除引擎数据失败: {e}"))?;
        Ok(bytes)
    })
    .await
    .map_err(|e| format!("删除任务失败: {e}"))?
}

/// 立即执行搬家（用户在设置页确认后调用）。只认 Migratable，Conflict 一律拒绝。
/// 返回给界面一句话说明搬了多少、原件在哪。
#[tauri::command]
pub async fn migrate_storage_now(app: AppHandle) -> Result<String, String> {
    let paths = app.state::<StoragePaths>().inner().clone();
    match migrate::detect_in(&paths) {
        StorageState::Migratable => {}
        StorageState::Conflict => {
            return Err("新旧两处都有数据，不能替你选一边：请在设置页逐项确认处置方式".to_string())
        }
        _ => return Err("没有需要迁移的旧位置数据".to_string()),
    }
    let stamp = chrono::Utc::now().format("%Y%m%d").to_string();
    let legacy_root = paths.config_root.clone();
    let content_root = paths.content_root.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        migrate::migrate_items(&legacy_root, &content_root, &stamp)
    })
    .await
    .map_err(|e| format!("迁移任务失败: {e}"))??;

    // 设计 §四 step 4：搬完要落指针，下次启动即便默认位置推导不出这个根也认它
    let pointer = paths::pointer_json(&paths.content_root, Some(&paths.config_root.display().to_string()));
    crate::storage::atomic::atomic_write(&paths.pointer_file(), pointer.as_bytes())
        .map_err(|e| format!("写存储指针失败（数据已在新位置，可重试点一次）: {e}"))?;

    let mb = report.bytes as f64 / 1024.0 / 1024.0;
    log_info!("存储迁移完成：{} 项 / {:.1} MB，原件在 {}", report.moved, mb, report.backup.display());
    Ok(format!(
        "已迁移 {} 项 / {:.1} MB，原件在 {}",
        report.moved,
        mb,
        report.backup.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 残留数据是plugin_data与注册表差集() {
        let dir = tempfile::tempdir().unwrap();
        let pd = dir.path().join("data").join("plugin-data");
        std::fs::create_dir_all(pd.join("kept-plugin")).unwrap();
        std::fs::create_dir_all(pd.join("gone-plugin")).unwrap();
        let installed = ["kept-plugin".to_string()];
        let orphans = orphan_plugin_data(&pd, &installed).unwrap();
        assert_eq!(orphans, vec!["gone-plugin".to_string()]);
    }

    #[test]
    fn 未创建目录时报告为空而不是报错() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope");
        assert!(orphan_plugin_data(&missing, &[]).unwrap().is_empty());
    }

    #[test]
    fn 只把插件目录当残留() {
        // 隐藏目录、散文件不能出现在「可删除的引擎数据」里，
        // 否则用户点一下就删掉迁移原件备份之类的东西
        let dir = tempfile::tempdir().unwrap();
        let pd = dir.path().join("plugin-data");
        std::fs::create_dir_all(pd.join(".hidden")).unwrap();
        std::fs::create_dir_all(pd.join("real-plugin")).unwrap();
        std::fs::write(pd.join("loose.txt"), b"x").unwrap();
        assert_eq!(
            orphan_plugin_data(&pd, &[]).unwrap(),
            vec!["real-plugin".to_string()]
        );
    }

    #[test]
    fn 插件数据体积按目录递归求和而目录缺失给零() {
        // 卸载弹窗要拿这个数告诉用户“能省 3.2 GB”，查不到时给 0 而不是报错，
        // 否则扫描失败会把卸载流程卡住
        let dir = tempfile::tempdir().unwrap();
        let content = dir.path();
        let d = paths::plugin_data_dir(content, "kept-plugin");
        std::fs::create_dir_all(d.join("model")).unwrap();
        std::fs::write(d.join("model").join("w.bin"), vec![7u8; 4096]).unwrap();

        let p = StoragePaths::from_dirs(
            content.to_path_buf(),
            content.to_path_buf(),
            content.join("plugins"),
            false,
        );
        assert_eq!(plugin_data_bytes(&p, "kept-plugin"), 4096);
        assert_eq!(plugin_data_bytes(&p, "nope-plugin"), 0, "没建过数据不该报错");
    }

    #[test]
    fn 非法插件id拒绝删除() {
        // 删目录的命令拿到的是前端字符串，必须挡住穿越与空值
        for bad in ["", "../evil", "a/b", "带中文", ".hidden"] {
            assert!(check_plugin_id(bad).is_err(), "应拒绝 id={bad:?}");
        }
        assert!(check_plugin_id("some-plugin").is_ok());
        assert!(check_plugin_id("underscore_plugin").is_ok());
    }

    #[test]
    fn 迁移状态只对外暴露三种标签() {
        // 前后端契约字符串，前端按它决定按钮文案，取值不得漂移
        assert_eq!(migration_tag(StorageState::Current), "none");
        assert_eq!(migration_tag(StorageState::Fresh), "none");
        assert_eq!(migration_tag(StorageState::Migratable), "migratable");
        assert_eq!(migration_tag(StorageState::Conflict), "conflict");
    }

    #[test]
    fn 存储报告带首启标记插件根与磁盘信息() {
        // 前端「存储」页与首启弹窗的跨线契约字段，少一个 B 线就编译不过
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let content = dir.path().join("data");
        let plugins = dir.path().join("plugins");
        let exe_data = dir.path().join("exe").join("data");
        std::fs::create_dir_all(&exe_data).unwrap();
        std::fs::write(exe_data.join("messages.json"), b"12345").unwrap();

        let mut p = StoragePaths::from_dirs(cfg, content, plugins.clone(), false);
        p.legacy_exe_data = Some(exe_data);
        let drives = vec![DriveInfo { letter: "C:".into(), free_bytes: 100, total_bytes: 200 }];
        let r = build_report_with_drives(&p, &[], true, drives);

        assert!(r.storage_choice_made);
        assert_eq!(r.plugins_root, plugins.display().to_string());
        assert!(!r.plugins_fallback);
        assert_eq!(r.legacy_exe_data_bytes, 5, "1.9.0 测试版留在 <exe>\\data 的数据要能统计到");
        assert_eq!(r.drives[0].letter, "C:");
        assert_eq!(r.drives[0].free_bytes, 100);
    }

    #[test]
    fn 旧exe数据与内容根相同时不重复统计() {
        // 内容根当前仍默认跟随安装目录时，<exe>\data 就是内容根本身，
        // 不能把它当成“另一处旧数据”提示用户迁移。
        let dir = tempfile::tempdir().unwrap();
        let content = dir.path().join("data");
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(content.join("messages.json"), b"12345").unwrap();
        let mut p = StoragePaths::from_dirs(
            dir.path().join("cfg"),
            content.clone(),
            dir.path().join("plugins"),
            false,
        );
        p.legacy_exe_data = Some(content);
        let r = build_report_with_drives(&p, &[], false, Vec::new());
        assert_eq!(r.legacy_exe_data_bytes, 0);
        assert!(!r.storage_choice_made);
    }

    #[test]
    fn move_plan_列出待搬项且跳过目标已存在() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("messages.json"), b"[1]").unwrap();
        std::fs::write(src.path().join("favorites.json"), b"[7]").unwrap();
        std::fs::create_dir_all(src.path().join(paths::AUDIO_DIR_NAME)).unwrap();
        std::fs::write(src.path().join(paths::AUDIO_DIR_NAME).join("a.wav"), b"x").unwrap();
        std::fs::write(dst.path().join("favorites.json"), b"[9]").unwrap(); // 目标已有 → 不得覆盖

        let plan = build_move_plan(src.path(), dst.path());
        assert!(plan.items.iter().any(|i| i.rel == "messages.json"));
        assert!(plan.items.iter().any(|i| i.rel == "audio/a.wav"));
        assert!(plan.skipped.iter().any(|s| s.contains("favorites.json")), "目标已存在的文件必须跳过而非覆盖");
        assert!(plan.total_bytes > 0);
    }

    #[test]
    fn move_plan_源不存在时为空且不报错() {
        let dst = tempfile::tempdir().unwrap();
        let plan = build_move_plan(std::path::Path::new(r"Z:\不存在的目录"), dst.path());
        assert!(plan.items.is_empty());
        assert!(plan.total_bytes == 0);
    }

    #[test]
    fn move_plan_不搬临时文件() {
        // cache/downloads/logs 是可重建的临时文件，搬家时不该跟着走（设计 D7）
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        for dir in ["cache", "downloads", "logs", "audio"] {
            std::fs::create_dir_all(src.path().join(dir)).unwrap();
            std::fs::write(src.path().join(dir).join("f.bin"), b"x").unwrap();
        }
        let plan = build_move_plan(src.path(), dst.path());
        let rels: Vec<&str> = plan.items.iter().map(|i| i.rel.as_str()).collect();
        assert!(rels.contains(&"audio/f.bin"));
        for temp in ["cache/f.bin", "downloads/f.bin", "logs/f.bin"] {
            assert!(!rels.contains(&temp), "临时文件不应搬：{temp}");
        }
    }

    #[test]
    fn move_content_复制校验后原件转备份() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let backup = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("messages.json"), b"[1,2]").unwrap();
        std::fs::create_dir_all(src.path().join(paths::AUDIO_DIR_NAME)).unwrap();
        std::fs::write(src.path().join(paths::AUDIO_DIR_NAME).join("a.wav"), b"abc").unwrap();

        let report = move_content(src.path(), dst.path(), backup.path()).unwrap();

        assert_eq!(report.moved_files, 2);
        assert_eq!(report.moved_bytes, 8, "5 字节 messages.json + 3 字节 a.wav");
        assert!(dst.path().join("messages.json").exists());
        assert!(dst.path().join(paths::AUDIO_DIR_NAME).join("a.wav").exists());
        assert!(!src.path().join("messages.json").exists(), "搬完原件应从旧位置移走");
        assert!(backup.path().join("messages.json").exists(), "原件进备份，可回退");
        assert!(backup.path().join(paths::AUDIO_DIR_NAME).join("a.wav").exists());
    }

    #[test]
    fn move_content_目标已有同名绝不覆盖() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let backup = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("favorites.json"), b"new").unwrap();
        std::fs::write(dst.path().join("favorites.json"), b"old").unwrap();

        let report = move_content(src.path(), dst.path(), backup.path()).unwrap();

        assert_eq!(report.moved_files, 0);
        assert_eq!(std::fs::read(dst.path().join("favorites.json")).unwrap(), b"old", "目标已有内容不得被覆盖");
        assert!(src.path().join("favorites.json").exists(), "跳过项留在原处，不塞进备份");
        assert!(report.skipped.iter().any(|s| s == "favorites.json"));
    }

    #[test]
    fn move_content_重复执行幂等() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let backup = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("messages.json"), b"[1]").unwrap();

        let first = move_content(src.path(), dst.path(), backup.path()).unwrap();
        let second = move_content(src.path(), dst.path(), backup.path()).unwrap();

        assert_eq!(first.moved_files, 1);
        assert_eq!(second.moved_files, 0, "第二次没有可搬的东西");
        assert_eq!(std::fs::read(dst.path().join("messages.json")).unwrap(), b"[1]");
    }

    #[test]
    fn legacy_exe_data_仅在默认位置缺messages时判定为可回迁() {
        // 1.9.0 测试版把数据放 <exe>\data；回迁前提是默认位置还没有消息，
        // 否则就是把测试版旧数据盖到用户升级后新写的数据上。
        let exe_data = tempfile::tempdir().unwrap();
        let content = tempfile::tempdir().unwrap();
        std::fs::write(exe_data.path().join("messages.json"), b"[1,2]").unwrap();
        assert!(should_pull_back_legacy(exe_data.path(), content.path()), "默认位置空 → 应回迁");
        std::fs::write(content.path().join("messages.json"), b"[9]").unwrap();
        assert!(
            !should_pull_back_legacy(exe_data.path(), content.path()),
            "默认位置已有数据 → 不得回迁覆盖"
        );
    }

    #[test]
    fn 用户自选位置时不回迁测试版数据() {
        // 三条件之「用户没自选位置」：content_root != config_root 时，数据不能搬到他没选的地方。
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let chosen = dir.path().join("D-data");
        let exe_data = dir.path().join("exe").join("data");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&chosen).unwrap();
        std::fs::create_dir_all(&exe_data).unwrap();
        std::fs::write(exe_data.join("messages.json"), b"[1]").unwrap();

        let mut custom = StoragePaths::from_dirs(
            cfg.clone(),
            chosen.clone(),
            dir.path().join("plugins"),
            false,
        );
        custom.legacy_exe_data = Some(exe_data.clone());
        assert!(pull_back_legacy_if_needed(&custom).is_none());
        assert!(exe_data.join("messages.json").exists(), "自选位置时不得搬动测试版数据");
        assert!(!chosen.join("messages.json").exists());

        // 默认位置（content == config）→ 回迁并给出通知，原件转备份
        let mut default = StoragePaths::from_dirs(
            cfg.clone(),
            cfg.clone(),
            dir.path().join("plugins"),
            false,
        );
        default.legacy_exe_data = Some(exe_data.clone());
        let notice = pull_back_legacy_if_needed(&default).expect("应回迁并返回通知");
        assert!(notice.contains("测试版"), "通知要说明数据从哪来：{notice}");
        assert!(!exe_data.join("messages.json").exists(), "回迁后原件应收进备份");
        assert_eq!(std::fs::read(cfg.join("messages.json")).unwrap(), b"[1]");
        let backups = migrate::find_backups(&cfg);
        assert_eq!(backups.len(), 1, "原件必须留一份备份");
        assert!(backups[0].join("messages.json").exists());
    }
}
