// 首启数据检测与可回退迁移（设计文档 §四）。
//
// 为什么放在启动而不是安装器里：NSIS 里搬几 GB + 校验 + 回滚是事故温床；放首启则天然幂等、
// 有界面可交互、失败可重试。1.9 及以后每版启动都跑这道判定，即用户要求的“装完先做数据检测”。

use std::path::{Path, PathBuf};

use super::paths::{
    StoragePaths, AUDIO_DIR_NAME, CACHE_DIR_NAME, DOWNLOADS_DIR_NAME, LOGS_DIR_NAME,
    SAMPLES_DIR_NAME,
};

/// 启动时的存储布局状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageState {
    /// 新布局已就位，什么也不做
    Current,
    /// 两边都没数据（全新安装）
    Fresh,
    /// 旧位置有数据、新位置空 → 可迁移（需用户确认）
    Migratable,
    /// 两边都有数据 → 必须让用户选，静默二选一就是丢数据
    Conflict,
}

impl Default for StorageState {
    /// 默认 Current：没把握时宁可不说“该搬家”，也不乱弹迁移提示
    fn default() -> Self {
        StorageState::Current
    }
}

/// 迁移状态持有器已下线：`storage_report` 直接算实时状态（见 `detect_in`），
/// 启动时缓存一份再给界面读就会出现两处真相——用户搬完家后快照还是 Migratable。

/// 判定当前处于哪种状态。
pub fn detect_state(content_has_data: bool, legacy_present: bool) -> StorageState {
    match (content_has_data, legacy_present) {
        (false, false) => StorageState::Fresh,
        (true, false) => StorageState::Current,
        (false, true) => StorageState::Migratable,
        (true, true) => StorageState::Conflict,
    }
}

/// 内容类条目清单：**迁移与回退共用同一份**（两个方向各自维护的话，改一边漏一边）。
/// **不含** `settings.json` / `remote-pairing.json` / `storage.json`：配置留在配置根，
/// 把它们搬走会让下一次启动读不到配置（新旧布局共用同一个配置根目录）。
const CONTENT_FILES: [&str; 3] = ["messages.json", "favorites.json", "subtitle_history.json"];
const CONTENT_DIRS: [&str; 5] = [
    AUDIO_DIR_NAME,
    SAMPLES_DIR_NAME,
    LOGS_DIR_NAME,
    CACHE_DIR_NAME,
    DOWNLOADS_DIR_NAME,
];

/// 待迁移清单 = 内容类条目。
pub fn legacy_items(legacy_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for f in CONTENT_FILES {
        let p = legacy_root.join(f);
        if p.is_file() {
            out.push(p);
        }
    }
    for d in CONTENT_DIRS {
        let p = legacy_root.join(d);
        // 空目录不算“有数据”：否则一个创建失败的空 logs 会永久弹迁移提示
        if p.is_dir() && dir_has_entries(&p) {
            out.push(p);
        }
    }
    out
}

fn dir_has_entries(p: &Path) -> bool {
    std::fs::read_dir(p).map(|mut r| r.next().is_some()).unwrap_or(false)
}

/// 按当前布局判定状态——启动日志与 `storage_report` 共用这一份逻辑，不分两头写。
///
/// 两根仍相同时（内容根尚未翻默认到安装位置）根本没“搬家”这回事，必须返回 Current：
/// 否则 `messages.json` 会被同时看成“新位置已有”与“旧位置待迁”，误判成 Conflict。
///
/// 旧位置仍有数据时，**不能拿“新位置已有数据”当冲突依据**：拷贝就是先填新位置的，
/// 搬一半崩了新位置必然已有数据。设计 §五 承诺的“断电后重跑”要求这种情况仍判可迁移，
/// 只有「同名且内容不同」（两边各自有新数据）才真需要人来处置。
pub fn detect_in(paths: &StoragePaths) -> StorageState {
    if paths.content_root == paths.config_root {
        return StorageState::Current;
    }
    let legacy = legacy_items(&paths.config_root);
    if legacy.is_empty() {
        // 旧位置已清空：新位置有数据 = 已就位，没数据 = 全新安装
        return detect_state(content_has_data(&paths.content_root), false);
    }
    let (_, _, conflicts) = partition_items(&legacy, &paths.content_root);
    if conflicts.is_empty() {
        StorageState::Migratable
    } else {
        StorageState::Conflict
    }
}

/// 内容根是否已有内容类数据（判“新布局已就位”用）。
pub fn content_has_data(content_root: &Path) -> bool {
    content_root.join("messages.json").exists()
        || CONTENT_DIRS
            .iter()
            .any(|d| dir_has_entries(&content_root.join(d)))
}

/// 迁移结果。`backup` 是原件被移进去的目录（不删任何东西，可回退）。
#[derive(Debug, Default, Clone)]
pub struct MigrationReport {
    pub moved: usize,
    pub bytes: u64,
    pub backup: PathBuf,
}

/// 真实迁移：复制 + 逐项目自检（目录非空、文件字节数与源一致）。
pub fn migrate_items(legacy_root: &Path, content_root: &Path, stamp: &str) -> Result<MigrationReport, String> {
    migrate_items_with_verify(legacy_root, content_root, stamp, |_| true)
}

/// 先全部复制并逐项校验，**全通过才动原件**：
/// 任一项失败就删掉本次复制的半成品、原件原地不动，下次启动仍判 Migratable 可重跑。
/// `verify` 注入以便单测模拟“校验不通过”这种磁盘/复制异常。
pub fn migrate_items_with_verify(
    legacy_root: &Path,
    content_root: &Path,
    stamp: &str,
    verify: impl Fn(&Path) -> bool,
) -> Result<MigrationReport, String> {
    let backup = legacy_root.join(format!(".migrated-backup-{stamp}"));
    let items = legacy_items(legacy_root);
    let mut report = MigrationReport { moved: 0, bytes: 0, backup: backup.clone() };
    if items.is_empty() {
        return Ok(report);
    }

    // 先按“新位置现状”分类，再决定拷什么。不能直接拷：自动搬迁跑在后台，期间用户
    // 可能已经往内容根写了新消息，拿旧 `messages.json` 盖过去就是抹掉升级后的新数据。
    let (to_copy, already, conflicts) = partition_items(&items, content_root);
    if !conflicts.is_empty() {
        return Err(format!(
            "新位置已有同名且内容不同的文件，不会覆盖你升级后新写的数据，请先手工处置：{}",
            conflicts.join("、")
        ));
    }

    // 迁移是「同名平移」，源叶名即目标叶名（迁移侧不存在 `.N` 归档后缀那回事）
    let pairs: Vec<(PathBuf, PathBuf)> = to_copy
        .iter()
        .filter_map(|src| src.file_name().map(|n| (src.clone(), content_root.join(n))))
        .collect();
    let mut done = copy_all_and_verify(&pairs, verify)?;
    // 上一轮已拷成功但没改名成功的项：不重拷，但仍要把原件收进备份（否则下次仍判待迁移）
    for src in &already {
        if let Some(name) = src.file_name() {
            done.push((src.clone(), content_root.join(name)));
        }
    }
    for (src, _) in &done {
        report.bytes += path_size(src);
    }

    // 走到这里才创建备份目录并把原件移进去（失败也不删原件，下次仍会看到旧项）
    std::fs::create_dir_all(&backup).map_err(|e| format!("创建备份目录失败: {e}"))?;
    for (src, _) in done {
        let name = match src.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => continue,
        };
        // 归档目标必须现算：重跑/回退后重搬时，同名归档可能已在（上一轮留下的）。
        let target = free_backup_target(&backup, &name);
        match std::fs::rename(&src, &target) {
            Ok(()) => report.moved += 1,
            Err(e) => log_warn!("迁移原件入备份失败（数据已在新位置，原件保留）: {e}"),
        }
    }
    Ok(report)
}

/// 归档目标若已被占，改名存放而**不是盖上去**。
/// 必须这样写：Windows 的 `fs::rename` 带 REPLACE 语义，目标存在时不报错而是
/// **静默销毁旧归档**（实测就是这个 bug）——而归档是用户回退的唯一凭据。
fn free_backup_target(backup_dir: &Path, name: &str) -> PathBuf {
    let first = backup_dir.join(name);
    if !first.exists() {
        return first;
    }
    let mut i = 1usize;
    loop {
        let candidate = backup_dir.join(format!("{name}.{i}"));
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

/// 回退结果。`restored` 是复制回目标的项数（备份原件一律保留）。
#[derive(Debug, Default, Clone)]
pub struct RestoreReport {
    pub restored: usize,
    pub bytes: u64,
    pub backup: PathBuf,
}

/// 一条可回退项：`src` 是备份里的实际归档路径（可能是 `messages.json.1`），
/// `name` 是它放回目标时该用的**逻辑名**（`messages.json`）。两者必须分开——同日多次
/// 搬迁会把后一份归档改名成 `.1`/`.2`，把归档名直接拷回用户目录就成了脏名字。
#[derive(Debug, Clone)]
pub struct RestoreItem {
    pub src: PathBuf,
    pub name: String,
}

/// 把原始叶名归到它的内容类逻辑名上；不属于内容清单（如 `settings.json`）返回 None。
/// 允许尾随 `.<数字>` 归档后缀（`free_backup_target` 撞名时递增生成），非数字后缀一律不认。
fn logical_name(raw: &str) -> Option<&'static str> {
    for candidate in CONTENT_FILES.iter().chain(CONTENT_DIRS.iter()) {
        let logical: &'static str = candidate;
        if raw == logical {
            return Some(logical);
        }
        if let Some(rest) = raw.strip_prefix(logical) {
            if let Some(digits) = rest.strip_prefix('.') {
                if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                    return Some(logical);
                }
            }
        }
    }
    None
}

/// 归档后缀号：裸名记 0，`name.N` 记 N。后缀越大越新（`free_backup_target` 只在低位被占时递增）。
fn backup_suffix(raw: &str, logical: &str) -> usize {
    if raw == logical {
        return 0;
    }
    // logical 全 ASCII，字节切片安全；跳过 logical 本身与那个 '.'
    raw[logical.len() + 1..].parse().unwrap_or(0)
}

/// 备份目录里可回退的条目 = 内容清单，**同一逻辑名只取后缀最大（最新）的那一份**。
///
/// 为什么不能沿用 `legacy_items`（按精确名扫）：同日二次搬迁会把本轮归档改名成
/// `messages.json.1`，裸名 `messages.json` 反而是**更旧**的一份（Windows rename 不覆盖已有项）。
/// 按裸名回退 = 把用户早已删掉的消息塞回去——这正是 `--list` 实机跑出来的真缺陷。
/// 配置类不在清单里，所以回退也不会碰 `settings.json`。
pub fn backup_items(backup_root: &Path) -> Vec<RestoreItem> {
    let Ok(entries) = std::fs::read_dir(backup_root) else {
        return Vec::new();
    };
    // 先按逻辑名收集「后缀最大的那份」
    let mut best: Vec<(&'static str, usize, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let raw = match path.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => continue,
        };
        let Some(logical) = logical_name(&raw) else { continue };
        // 空目录不算可回退项，与 legacy_items 口径一致
        if path.is_dir() && !dir_has_entries(&path) {
            continue;
        }
        let suffix = backup_suffix(&raw, logical);
        match best.iter_mut().find(|b| b.0 == logical) {
            Some(slot) => {
                if suffix > slot.1 {
                    *slot = (logical, suffix, path);
                }
            }
            None => best.push((logical, suffix, path)),
        }
    }
    // 按固定清单顺序输出（FILES 再 DIRS），保证显示与回退顺序稳定
    let mut out = Vec::new();
    for logical in CONTENT_FILES.iter().chain(CONTENT_DIRS.iter()) {
        if let Some(slot) = best.iter().find(|b| &b.0 == logical) {
            out.push(RestoreItem { src: slot.2.clone(), name: (*logical).to_string() });
        }
    }
    out
}

/// 目标处已存在的同名条目。回退是“把当年搬走的放回去”，撞上同名 = 那边已有新数据，
/// 静默覆盖等于用旧消息记下覆盖新消息记下。**用逻辑名比对**，不是归档名。
pub fn restore_conflicts(backup_root: &Path, target_root: &Path) -> Vec<String> {
    backup_items(backup_root)
        .iter()
        .filter(|item| target_root.join(&item.name).exists())
        .map(|item| item.name.clone())
        .collect()
}

/// 把备份项复制回目标，**备份原样保留**（设计 §五：回退错了还能再试一次）。
/// 与 `migrate_items` 共用 `copy_all_and_verify`，不写第二份复制+校验。
/// 目标名一律用逻辑名：`messages.json.1` 这种归档名绝不能出现在用户目录里。
pub fn restore_items(backup_root: &Path, target_root: &Path) -> Result<RestoreReport, String> {
    let items = backup_items(backup_root);
    let conflicts = restore_conflicts(backup_root, target_root);
    if !conflicts.is_empty() {
        return Err(format!(
            "目标位置已有同名条目，为避免覆盖请先手工处置：{}",
            conflicts.join("、")
        ));
    }
    let pairs: Vec<(PathBuf, PathBuf)> = items
        .iter()
        .map(|it| (it.src.clone(), target_root.join(&it.name)))
        .collect();
    let done = copy_all_and_verify(&pairs, |_| true)?;
    let mut report = RestoreReport {
        restored: done.len(),
        bytes: 0,
        backup: backup_root.to_path_buf(),
    };
    for (src, _) in &done {
        report.bytes += path_size(src);
    }
    Ok(report)
}

/// 配置根下的迁移备份目录，按名字典序（目录名就是 `%Y%m%d`，末尾即最新）。
/// 只认目录且必须带备份前缀：散文件与 `audio` 这类止于内容目录的项不算备份。
pub fn find_backups(config_root: &Path) -> Vec<PathBuf> {
    const PREFIX: &str = ".migrated-backup-";
    let Ok(entries) = std::fs::read_dir(config_root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with(PREFIX))
                    .unwrap_or(false)
        })
        .collect();
    out.sort();
    out
}

/// 判两个文件是否同一内容。不能只看字节数：写错就是拿旧记录盖掉新记录。
/// 这条不是过度设计——开发时写的“内容不同”用例恰好与新文件同长（10 字节），
/// 只看长度的实现当场把它当成“已拷过”，冲突根本判不出来。
/// 参与判重的只有 `messages.json` 这类固定名的记录文件（几十到几百 KB），
/// 真比对内容的代价可以接受；大体积的目录走合并拷贝、不在这里比对。
fn same_file_contents(a: &Path, b: &Path) -> bool {
    let (Ok(x), Ok(y)) = (std::fs::read(a), std::fs::read(b)) else {
        return false;
    };
    x == y
}

/// 把待迁移项按新位置现状分成三类：还要拷 / 已拷过 / 撞名且不同（冲突）。
///
/// 只有**固定文件名的记录类文件**（`messages.json` 等）才参与判重：它们一边可能已有
/// 升级后新写的数据，盖过去就是丢数据。目录（`audio/` 等）不判重、直接合并拷：
/// 里面是按消息 id 命名的文件，id 不会撞，而“中途强杀后重跑”需要把上次没拷完的补上。
///
/// 判身份要**真比对文件内容**（见 `same_file_contents`），不能只看字节数。
/// 目的是既不让覆盖新数据、也不让重跑永远撞在自己留下的文件上。
fn partition_items(items: &[PathBuf], content_root: &Path) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<String>) {
    let mut to_copy = Vec::new();
    let mut already = Vec::new();
    let mut conflicts = Vec::new();
    for it in items {
        let Some(name) = it.file_name() else { continue };
        let dst = content_root.join(name);
        if dst.is_file() {
            if same_file_contents(it, &dst) {
                already.push(it.clone());
            } else {
                conflicts.push(name.to_string_lossy().into_owned());
            }
        } else {
            to_copy.push(it.clone());
        }
    }
    (to_copy, already, conflicts)
}

/// 复制 + 逐项校验的**公用段**：迁移与回退都走这里（设计 §五：“同一套复制+校验代码”）。
/// 任一项失败就把本次已复制的全删掉再报错，**源侧一律不动**。
/// 返回成功清单 `(源, 目标)`，调用方自己决定要不要把原件移走。
/// 收 `(源, 目标)` 对而不是「根 + 源列表」：回退的目标叶名是**逻辑名**（可能与源归档名
/// `messages.json.1` 不同），叶名推导不能只靠 `src.file_name()`，必须由调用方显式给出目标路径。
fn copy_all_and_verify(
    pairs: &[(PathBuf, PathBuf)],
    verify: impl Fn(&Path) -> bool,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (src, dst) in pairs {
        if let Some(parent) = dst.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                rollback(&done);
                return Err(format!("创建目标目录失败: {e}"));
            }
        }
        let copied = if src.is_dir() {
            crate::copy_dir_all(src, dst).map_err(|e| format!("复制目录失败: {e}"))
        } else {
            std::fs::copy(src, dst).map(|_| ()).map_err(|e| format!("复制文件失败: {e}"))
        };
        if let Err(e) = copied {
            discard(dst); // 复制中途失败也可能留下半截目录，它还没进 done，得单独清
            rollback(&done);
            return Err(e);
        }
        if !verify(dst) {
            discard(dst); // 校验不过的正是“当前项”，它尚未进 done，只靠 rollback 会留半成品
            rollback(&done);
            return Err(format!("复制校验失败，已回退：{}", dst.display()));
        }
        done.push((src.clone(), dst.clone()));
    }
    Ok(done)
}

/// 删掉单次复制产生的目标（目录递归删，文件直接删）；删不掉也不升级为错误——
/// 回退只是尽力收拾半成品，真正的原件从头到尾没动过。
fn discard(dst: &Path) {
    if dst.is_dir() {
        let _ = std::fs::remove_dir_all(dst);
    } else {
        let _ = std::fs::remove_file(dst);
    }
}

fn rollback(done: &[(PathBuf, PathBuf)]) {
    for (_, dst) in done.iter().rev() {
        discard(dst);
    }
}

/// 单项体积：目录递归求和，文件取自身字节数。
/// 不能直接交给 `paths::dir_size`——它的契约是「目录」，喂文件路径会因 read_dir
/// 失败返回 0，而待迁移清单里 messages.json 这类恰好是文件，UI 会少报体积。
/// pub：`storage_report` 统计旧位置体积要给界面看。
pub fn path_size(p: &Path) -> u64 {
    if p.is_dir() {
        super::paths::dir_size(p)
    } else {
        std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 状态判定四种组合() {
        assert_eq!(detect_state(false, false), StorageState::Fresh);
        assert_eq!(detect_state(true, false), StorageState::Current);
        assert_eq!(detect_state(false, true), StorageState::Migratable);
        assert_eq!(detect_state(true, true), StorageState::Conflict);
    }

    #[test]
    fn 旧项扫描只认清单里的条目() {
        let dir = tempfile::tempdir().unwrap();
        // settings.json 留在配置根、**不是**待迁移项（把它算进去会误判需要搬家）
        std::fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        assert!(legacy_items(dir.path()).is_empty(), "只有配置文件时不该搬家");
        assert!(!legacy_items(dir.path())
            .iter()
            .any(|p| p.display().to_string().contains("settings.json")));

        std::fs::create_dir_all(dir.path().join("audio")).unwrap();
        std::fs::write(dir.path().join("audio").join("m_1.wav"), b"x").unwrap();
        assert_eq!(legacy_items(dir.path()).len(), 1, "空目录不算待迁移项");

        std::fs::write(dir.path().join("messages.json"), b"[]").unwrap();
        assert_eq!(legacy_items(dir.path()).len(), 2);
    }

    #[test]
    fn 空目录不算待迁移项() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        assert!(legacy_items(dir.path()).is_empty(), "空 logs 目录不算有数据");
    }

    #[test]
    fn 迁移执行后原件进备份且记录可读() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(legacy.join("audio")).unwrap();
        std::fs::write(legacy.join("messages.json"), b"[]").unwrap();
        std::fs::write(legacy.join("audio").join("m_1.wav"), b"abc").unwrap();

        let report = migrate_items(&legacy, &content, "20261011").unwrap();

        assert_eq!(report.moved, 2);
        assert!(content.join("messages.json").exists());
        assert!(content.join("audio").join("m_1.wav").exists());
        // 原件不删：移入配置根下的 .migrated-backup-<stamp>，可随时回退
        assert!(report.backup.join("messages.json").exists());
        assert!(!legacy.join("messages.json").exists());
        assert_eq!(report.bytes, 5, "字节数按复制成功的文件累加");
    }

    #[test]
    fn 校验不过时一件都不搬原件也不动() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(legacy.join("audio")).unwrap();
        std::fs::write(legacy.join("messages.json"), b"[]").unwrap();
        std::fs::write(legacy.join("audio").join("m_1.wav"), b"abc").unwrap();

        // 注入一个永远校验失败的检查，模拟“复制不完整/落盘失败”
        let err = migrate_items_with_verify(&legacy, &content, "20261011", |_| false).unwrap_err();
        assert!(err.contains("校验失败"), "报错要说明原因：{err}");
        assert!(!content.join("messages.json").exists(), "校验失败不得留下半成品");
        assert!(!content.join("audio").join("m_1.wav").exists());
        assert!(legacy.join("messages.json").exists(), "原件必须原地不动");
        assert!(legacy.join("audio").join("m_1.wav").exists());
        assert!(
            !legacy.join(".migrated-backup-20261011").exists(),
            "回退时不该留下空的备份目录"
        );
    }

    #[test]
    fn 无待迁移项时是幂等空操作() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        std::fs::create_dir_all(&legacy).unwrap();
        let content = dir.path().join("data");
        let report = migrate_items(&legacy, &content, "20261011").unwrap();
        assert_eq!(report.moved, 0);
        assert_eq!(report.bytes, 0);
    }

    #[test]
    fn 两边都有但内容相同时仍判可重跑_真撞车才判冲突() {
        // 设计 §五 承诺“断电后重进仍可重跑”。但拷贝是先填新位置的，
        // 因此“新位置已有数据”本身不能当成冲突——否则搬一半崩了就会被误判成
        // Conflict（要人工处置），永远走不到重跑。只有**同名且内容不同**才是真撞车。
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cfg");
        let content = dir.path().join("data");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(&content).unwrap();
        let p = StoragePaths::from_dirs(cfg.clone(), content.clone(), dir.path().join("plugins"), false);

        // 旧位置有数据、新位置空 → 可迁移
        std::fs::write(cfg.join("messages.json"), b"same-bytes").unwrap();
        assert_eq!(detect_in(&p), StorageState::Migratable);

        // 两边都有、内容相同（断电重跑的真实形状）→ 仍应判可迁移，而不是冲突
        std::fs::write(content.join("messages.json"), b"same-bytes").unwrap();
        assert_eq!(detect_in(&p), StorageState::Migratable, "同内容不该算冲突");

        // 两边都有、同名但内容不同 → 这才是要人处置的冲突
        std::fs::write(content.join("messages.json"), b"different!").unwrap();
        assert_eq!(detect_in(&p), StorageState::Conflict);
    }

    #[test]
    fn 原件入备份遇同名时改名不覆盖不静默丢弃() {
        // 回退过又重搬（或同一天多次搬迁）时，备份目录里可能已有同名项。
        // Windows 上 rename 到已存在的文件会直接失败；若只记一条警告跳过，
        // 原件就留在旧位置 → 下次仍判待迁移 → **每次启动重搬一遍、永远不收敛**。
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("cfg");
        let content = dir.path().join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&content).unwrap();
        // 先造好同日备份目录，并往里放一份同名项（模拟上一轮已归档）
        let backup = legacy.join(".migrated-backup-20261011");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("messages.json"), b"archived").unwrap();
        // 旧位置待迁移项，与新位置内容相同（走“已拷过”分支）
        std::fs::write(legacy.join("messages.json"), b"same-bytes").unwrap();
        std::fs::write(content.join("messages.json"), b"same-bytes").unwrap();

        let report = migrate_items(&legacy, &content, "20261011").unwrap();

        assert_eq!(report.moved, 1, "原件必须被收走，否则迁移永不收敛");
        assert!(!legacy.join("messages.json").exists(), "旧位置不该还留着它");
        // 两份都在：上一轮的归档没被动过，这一轮的改名存放
        assert_eq!(std::fs::read(backup.join("messages.json")).unwrap(), b"archived", "已有归档不得被覆盖");
        let extra: Vec<String> = (std::fs::read_dir(&backup).unwrap())
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "messages.json")
            .collect();
        assert_eq!(extra.len(), 1, "本轮回退件应改名归档：{extra:?}");
        assert_eq!(std::fs::read(backup.join(&extra[0])).unwrap(), b"same-bytes");
    }

    #[test]
    fn 回退取同一逻辑名的最新归档而不是裸名那份() {
        // 同日二次搬迁会把本轮归档改名成 `messages.json.1`，裸名留下的反而是**更旧**的一份
        //（裸名是第一次搬迁归档的，Windows rename 不覆盖已有项）。
        // 按裸名回退 = 把用户早已删掉的消息列表塞回去，所以必须取后缀最大的那份。
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join(".migrated-backup-20261011");
        let target = dir.path().join("cfg");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("messages.json"), b"older-list").unwrap();
        std::fs::write(backup.join("messages.json.1"), b"newest-list").unwrap();

        let items = backup_items(&backup);
        assert_eq!(items.len(), 1, "同一逻辑名的多份归档只能算一项");
        assert_eq!(items[0].name, "messages.json", "回退时必须用逻辑名，不能叫 messages.json.1");

        let report = restore_items(&backup, &target).unwrap();
        assert_eq!(report.restored, 1);
        assert_eq!(
            std::fs::read(target.join("messages.json")).unwrap(),
            b"newest-list",
            "必须拿最新归档，目标处也不能出现 messages.json.1 这种名字"
        );
        assert!(!target.join("messages.json.1").exists());
    }

    #[test]
    fn 迁移遇新位置已有同名不同内容时整批拒绝() {
        // 自动搬迁跑在后台，期间用户可能已经发了新消息（内容根的 messages.json 已存在且不同）。
        // 这是唯一会**真丢数据**的情形：拿旧文件盖过去就是抹掉用户升级后写的东西。
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(legacy.join("messages.json"), b"old").unwrap();
        std::fs::write(content.join("messages.json"), b"new-longer").unwrap();

        let err = migrate_items(&legacy, &content, "20261011").unwrap_err();
        assert!(err.contains("messages.json"), "要列出撞了哪一项：{err}");
        assert_eq!(std::fs::read(content.join("messages.json")).unwrap(), b"new-longer", "不得覆盖新数据");
        assert!(legacy.join("messages.json").exists(), "原件不动");
        assert!(!legacy.join(".migrated-backup-20261011").exists(), "拒绝时不该造备份目录");
    }

    #[test]
    fn 中途强杀后重跑_相同文件不重复拷但仍入备份() {
        // 幂等重跑是验收硬要求（设计 §五：断电后重进仍判 Migratable）。上一轮可能已把
        // messages.json 拷过去了、但改名失败没成功——重跑时“目标已存在”不能当冲突，
        // 否则永远搬不完。
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("legacy");
        let content = dir.path().join("data");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&content).unwrap();
        std::fs::write(legacy.join("messages.json"), b"same-bytes").unwrap();
        std::fs::write(content.join("messages.json"), b"same-bytes").unwrap();

        let report = migrate_items(&legacy, &content, "20261011").unwrap();
        assert_eq!(report.moved, 1, "已搬过的也要把原件收进备份，不然下次仍判待迁移");
        assert!(!legacy.join("messages.json").exists());
        assert!(report.backup.join("messages.json").exists());
        assert_eq!(std::fs::read(content.join("messages.json")).unwrap(), b"same-bytes");
    }

    #[test]
    fn 回退把备份项复制回目标且备份保留() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join(".migrated-backup-20261011");
        let target = dir.path().join("cfg");
        std::fs::create_dir_all(backup.join("audio")).unwrap();
        std::fs::write(backup.join("messages.json"), b"[]").unwrap();
        std::fs::write(backup.join("audio").join("m_1.wav"), b"abc").unwrap();
        // 备份里的配置文件不在内容清单里，不该被当作可回退项（现在也不该少搬）
        std::fs::write(backup.join("settings.json"), b"{}").unwrap();

        let report = restore_items(&backup, &target).unwrap();

        assert_eq!(report.restored, 2, "只搬内容类条目");
        assert_eq!(report.bytes, 5);
        assert!(target.join("messages.json").exists());
        assert!(target.join("audio").join("m_1.wav").exists());
        assert!(!target.join("settings.json").exists(), "配置不进回退清单");
        // 设计 §五：备份保留，回退错了还能再试
        assert!(backup.join("messages.json").exists(), "备份原件不得动");
    }

    #[test]
    fn 回退遇同名条目整批拒绝() {
        // 回退是“把当年搬走的放回去”，目标处已有同名文件 = 已有新数据，
        // 静默覆盖等于用旧消息记下覆盖新消息记下
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("backup");
        let target = dir.path().join("cfg");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(backup.join("messages.json"), b"old").unwrap();
        std::fs::write(target.join("messages.json"), b"new").unwrap();

        let err = restore_items(&backup, &target).unwrap_err();
        assert!(err.contains("messages.json"), "报错要列出撞上哪几项：{err}");
        assert_eq!(std::fs::read(target.join("messages.json")).unwrap(), b"new", "不得覆盖");
    }

    #[test]
    fn 备份目录只认备份前缀且按日期排序() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".migrated-backup-20261012")).unwrap();
        std::fs::create_dir_all(dir.path().join(".migrated-backup-20261011")).unwrap();
        std::fs::create_dir_all(dir.path().join("audio")).unwrap();
        std::fs::write(dir.path().join(".migrated-backup-notastamp"), b"x").unwrap();

        let found = find_backups(dir.path());
        assert_eq!(found.len(), 2, "只应找到两个备份目录");
        // 字典序 == 时间序（目录名就是 %Y%m%d），取最新就是取末尾。
        // 注意用 file_name 比：PathBuf::ends_with 比的是路径成分，“20261011”不是独立成分会误判
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names[0], ".migrated-backup-20261011");
        assert_eq!(names[1], ".migrated-backup-20261012");
    }
}
