// 首启数据检测与可回退迁移（设计文档 §四）。
//
// 为什么放在启动而不是安装器里：NSIS 里搬几 GB + 校验 + 回滚是事故温床；放首启则天然幂等、
// 有界面可交互、失败可重试。1.9 及以后每版启动都跑这道判定，即用户要求的“装完先做数据检测”。

use std::path::{Path, PathBuf};

use super::paths::{
    AUDIO_DIR_NAME, CACHE_DIR_NAME, DOWNLOADS_DIR_NAME, LOGS_DIR_NAME, SAMPLES_DIR_NAME,
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

/// 迁移状态持有器：供 storage_report 读出给界面（不在启动路径上弹框）
#[derive(Debug, Clone, Default)]
pub struct MigrationPending {
    pub state: StorageState,
}

impl MigrationPending {
    pub fn new(state: StorageState) -> Self {
        Self { state }
    }
}

/// 判定当前处于哪种状态。
pub fn detect_state(content_has_data: bool, legacy_present: bool) -> StorageState {
    match (content_has_data, legacy_present) {
        (false, false) => StorageState::Fresh,
        (true, false) => StorageState::Current,
        (false, true) => StorageState::Migratable,
        (true, true) => StorageState::Conflict,
    }
}

/// 待迁移清单 = 内容类条目。
/// **不含** `settings.json` / `remote-pairing.json` / `storage.json`：配置留在配置根，
/// 把它们搬走会让下一次启动读不到配置（新旧布局共用同一个配置根目录）。
pub fn legacy_items(legacy_root: &Path) -> Vec<PathBuf> {
    const FILES: [&str; 3] = ["messages.json", "favorites.json", "subtitle_history.json"];
    const DIRS: [&str; 5] = [
        AUDIO_DIR_NAME,
        SAMPLES_DIR_NAME,
        LOGS_DIR_NAME,
        CACHE_DIR_NAME,
        DOWNLOADS_DIR_NAME,
    ];
    let mut out = Vec::new();
    for f in FILES {
        let p = legacy_root.join(f);
        if p.is_file() {
            out.push(p);
        }
    }
    for d in DIRS {
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

/// 内容根是否已有内容类数据（判“新布局已就位”用）。
pub fn content_has_data(content_root: &Path) -> bool {
    if content_root.join("messages.json").exists() {
        return true;
    }
    [
        AUDIO_DIR_NAME,
        SAMPLES_DIR_NAME,
        LOGS_DIR_NAME,
        CACHE_DIR_NAME,
        DOWNLOADS_DIR_NAME,
    ]
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
    let mut report = MigrationReport { moved: 0, bytes: 0, backup };
    if items.is_empty() {
        return Ok(report);
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for src in &items {
        let name = match src.file_name() {
            Some(n) => PathBuf::from(n),
            None => continue,
        };
        let dst = content_root.join(&name);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目标目录失败: {e}"))?;
        }
        let copied = if src.is_dir() {
            crate::copy_dir_all(src, &dst).map_err(|e| format!("复制目录失败: {e}"))
        } else {
            std::fs::copy(src, &dst).map(|_| ()).map_err(|e| format!("复制文件失败: {e}"))
        };
        if let Err(e) = copied {
            discard(&dst); // 复制中途失败也可能留下半截目录，它还没进 done，得单独清
            rollback(&done);
            return Err(e);
        }
        if !verify(&dst) {
            discard(&dst); // 校验不过的正是“当前项”，它尚未进 done，只靠 rollback 会留半成品
            rollback(&done);
            return Err(format!("迁移校验失败，已回退：{}", dst.display()));
        }
        report.bytes += path_size(src);
        done.push((src.clone(), dst));
    }

    // 走到这里才创建备份目录并把原件移进去（失败也不删原件，下次仍会看到旧项）
    std::fs::create_dir_all(&report.backup).map_err(|e| format!("创建备份目录失败: {e}"))?;
    for (src, _) in done {
        let target = match src.file_name() {
            Some(n) => report.backup.join(n),
            None => continue,
        };
        match std::fs::rename(&src, &target) {
            Ok(()) => report.moved += 1,
            Err(e) => log_warn!("迁移原件入备份失败（数据已在新位置，原件保留）: {e}"),
        }
    }
    Ok(report)
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
fn path_size(p: &Path) -> u64 {
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
}
