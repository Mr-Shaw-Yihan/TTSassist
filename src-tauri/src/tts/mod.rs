// TTS 引擎模块入口。
// 按大纲先实现 MiMo 引擎，接口在 traits.rs 留扩展位以备后续接入本地引擎。
// edge 引擎已抽出为插件（plugins/edge-tts），由插件系统加载。

pub mod traits;
pub mod mimo;
pub mod moss;

use std::path::Path;
use traits::{TTSEngine, TtsError};
use crate::storage::types::Settings;
use mimo::MimoEngine;

/// 根据 settings 构建引擎实例。
///
/// **注意**：每次调用都会新建引擎实例（无状态引擎），不缓存。
/// 插件引擎（如 edge-tts）不走这里，由 plugins::PluginManager 提供。
pub fn build_engine(settings: &Settings, data_dir: &Path) -> Result<Box<dyn TTSEngine>, TtsError> {
    match settings.tts_engine.as_str() {
        "mimo" => Ok(Box::new(MimoEngine::new(
            settings.mimo_api_key.clone(),
            data_dir.to_path_buf(),
        ))),
        other => Err(TtsError::UnknownEngine(other.into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::types::Settings;
    use tempfile::tempdir;

    #[test]
    fn build_mimo返回正确() {
        let dir = tempdir().unwrap();
        let s = Settings { tts_engine: "mimo".into(), mimo_api_key: "key".into(), ..Default::default() };
        let engine = build_engine(&s, dir.path()).unwrap();
        assert_eq!(engine.name(), "mimo");
    }

    #[test]
    fn 未知引擎返回错误() {
        let dir = tempdir().unwrap();
        let s = Settings { tts_engine: "nope".into(), ..Default::default() };
        let result = build_engine(&s, dir.path());
        assert!(result.is_err(), "未知引擎应返回错误");
        // 注意：Box<dyn TTSEngine> 不含 Debug，故测试只断言 is_err
    }
}