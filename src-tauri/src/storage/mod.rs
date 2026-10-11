// 存储层模块入口。
// 对外暴露子模块 + 数据目录定位/初始化辅助。

pub mod atomic;
pub mod audio_gc;
pub mod favorites;
pub mod messages;
pub mod migrate;
pub mod paths;
pub mod settings;
pub mod types;

pub use atomic::StorageError;

use std::path::Path;
use atomic::Result;

/// 确保内容根与其下的内容类子目录存在（不存在则创建，幂等）。
/// 子目录名字只来自 `paths`（存储契约：路径不得在业务代码里拼字面量）。
/// 通常在应用启动时调用一次。
/// 注意：阶段 22 起 plugins 目录不再在此创建（位于 exe 同级，由 PluginManager 管理）。
pub fn ensure_data_dirs(content_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(content_dir)?;
    std::fs::create_dir_all(paths::audio_dir_in(content_dir))?;
    std::fs::create_dir_all(paths::samples_dir_in(content_dir))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_创建目录和audio子目录() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("VoiceAssist");
        assert!(!target.exists());
        ensure_data_dirs(&target).unwrap();
        assert!(target.exists(), "数据目录被创建");
        assert!(target.join("audio").exists(), "audio 子目录被创建");
    }

    #[test]
    fn ensure_目录已存在不报错() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("VoiceAssist");
        ensure_data_dirs(&target).unwrap();
        ensure_data_dirs(&target).unwrap(); // 重复创建幂等
    }
}