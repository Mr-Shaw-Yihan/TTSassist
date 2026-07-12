// 存储层模块入口。
// 对外暴露子模块 + 数据目录定位/初始化辅助。

pub mod atomic;
pub mod audio_gc;
pub mod favorites;
pub mod messages;
pub mod settings;
pub mod types;

pub use atomic::StorageError;

use std::path::Path;
use atomic::Result;

/// 确保数据目录和 audio 子目录存在（不存在则创建）。
/// 通常在应用启动时调用一次。
pub fn ensure_data_dirs(data_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::create_dir_all(data_dir.join("audio"))?;
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