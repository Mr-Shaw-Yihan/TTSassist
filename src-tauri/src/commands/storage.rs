// 「存储」页与卸载弹窗的后端读口：谁占了多大、哪些是残留、能不能搬家。
//
// 设计文档 §六：体积统计是递归求和，大目录可达数万文件，因此三条命令一律走
// blocking 线程，不占 Tauri 主 runtime；扫描失败宁可报空也不要让整个页面打不开。
//
// 路径一律向 `StoragePaths` 要，不在这里拼字面量（AGENTS.md 存储契约）。

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};

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
pub fn build_report(paths: &StoragePaths, installed: &[String]) -> StorageReport {
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
    }
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
    tauri::async_runtime::spawn_blocking(move || build_report(&paths, &installed))
        .await
        .map_err(|e| format!("存储扫描任务失败: {e}"))
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
}
