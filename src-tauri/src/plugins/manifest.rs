// 插件清单（manifest.json）：每个插件目录里的"说明书"。
//
// 加载前宿主先读它、校验它（类型/平台/最低版本/id 合法性），
// 并按其中的 checksum 对 plugin.dll 做 SHA-256 完整性校验。

use std::path::Path;
use serde::{Deserialize, Serialize};
use super::PluginError;

/// manifest.json 结构（与 doc/插件系统规划.md §二 一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// 插件唯一 id（须与 dll 自报的 va_plugin_id 一致；只允许字母/数字/-/_）
    pub id: String,
    /// 展示名（如 "Edge TTS（免费·微软）"）
    pub name: String,
    /// 插件版本（如 "1.0.0"）
    pub version: String,
    /// 插件类型，当前只有 "tts_engine"（JSON 键名为 type）
    #[serde(rename = "type")]
    pub plugin_type: String,
    /// 支持平台列表（如 ["windows"]）
    pub platform: Vec<String>,
    /// 动态库文件名（如 "plugin.dll"）
    pub entry: String,
    /// 所需宿主最低版本（如 "1.3.0"）
    pub min_app_version: String,
    /// entry 文件的 SHA-256（十六进制小写），安装/加载时校验防篡改
    pub checksum: String,
    /// 描述（可选）
    #[serde(default)]
    pub description: String,
    /// 引擎类别（可选）："local"（本地离线）或 "remote"（联网），默认 remote。
    /// 本地插件（如本地推理引擎）应声明 "local"，UI 据此展示「本地·离线」标识。
    #[serde(default = "default_category")]
    pub category: String,
    /// 单次合成超时秒数（可选），默认 60。本地引擎首次推理含模型加载，
    /// 可声明更大值（如 1200 = 20 分钟，覆盖首次运行环境下载引导）。
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// 资源需求说明（可选，人类可读）：下载体积/内存占用/CPU 要求等，
    /// 供用户在下载安装前判断本机配置是否够用。本地推理引擎建议填写。
    #[serde(default)]
    pub requirements: Option<String>,
}

fn default_category() -> String {
    "remote".to_string()
}

fn default_timeout_secs() -> u64 {
    60
}

impl PluginManifest {
    /// 从插件目录读取并解析 manifest.json
    pub fn load(plugin_dir: &Path) -> Result<Self, PluginError> {
        let path = plugin_dir.join("manifest.json");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| PluginError::NotFound(format!("清单文件不存在: {} ({e})", path.display())))?;
        // PowerShell 等工具写出的 UTF-8 文件可能带 BOM，serde_json 不认，先剥掉
        let raw = raw.trim_start_matches('\u{FEFF}');
        serde_json::from_str(raw)
            .map_err(|e| PluginError::Manifest(format!("manifest.json 解析失败: {e}")))
    }

    /// 加载前校验：id 合法 + 类型匹配 + 平台匹配 + 宿主版本达标
    pub fn validate(&self, app_version: &str) -> Result<(), PluginError> {
        // id 必须是安全目录名（防路径穿越/奇怪字符）
        if self.id.is_empty()
            || self.id.len() > 64
            || !self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(PluginError::Unsupported(format!(
                "插件 id「{}」不合法（只允许字母、数字、-、_，最长 64）",
                self.id
            )));
        }
        if self.plugin_type != "tts_engine" && self.plugin_type != "asr_engine" {
            return Err(PluginError::Unsupported(format!(
                "不支持的插件类型「{}」（当前支持 tts_engine / asr_engine）",
                self.plugin_type
            )));
        }
        if !self.platform.iter().any(|p| p == "windows") {
            return Err(PluginError::Unsupported(format!(
                "插件「{}」不支持 Windows 平台",
                self.id
            )));
        }
        if version_less_than(app_version, &self.min_app_version) {
            return Err(PluginError::Unsupported(format!(
                "插件「{}」需要宿主 ≥ {}，当前宿主版本 {}",
                self.id, self.min_app_version, app_version
            )));
        }
        if self.checksum.trim().is_empty() {
            return Err(PluginError::Unsupported(format!(
                "插件「{}」缺少 checksum，拒绝加载",
                self.id
            )));
        }
        Ok(())
    }
}

/// 简易版本号比较：a < b 返回 true。只按数字段逐段比较（1.3.0 / 1.10.2 均可），
/// 非数字段当 0 处理（不支持预发布标签，本项目版本均为 x.y.z）。
pub fn version_less_than(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.split('.')
            .map(|seg| seg.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .map(|num| num.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (mut va, mut vb) = (parse(a), parse(b));
    // 补齐到等长（1.3 == 1.3.0）
    let max_len = va.len().max(vb.len());
    va.resize(max_len, 0);
    vb.resize(max_len, 0);
    va < vb
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PluginManifest {
        PluginManifest {
            id: "edge-tts".into(),
            name: "Edge TTS".into(),
            version: "1.0.0".into(),
            plugin_type: "tts_engine".into(),
            platform: vec!["windows".into()],
            entry: "plugin.dll".into(),
            min_app_version: "1.3.0".into(),
            checksum: "abc123".into(),
            description: String::new(),
            category: default_category(),
            timeout_secs: default_timeout_secs(),
            requirements: None,
        }
    }

    #[test]
    fn 正常清单通过校验() {
        assert!(sample().validate("1.3.1").is_ok());
        assert!(sample().validate("2.0.0").is_ok());
    }

    #[test]
    fn 宿主版本过低拒绝() {
        let err = sample().validate("1.2.9").unwrap_err();
        assert!(err.to_string().contains("需要宿主"));
    }

    #[test]
    fn 版本比较_按数值而非字符串() {
        // 字符串比较 "1.10" < "1.9"，数值比较必须反过来
        assert!(!version_less_than("1.10.0", "1.9.0"));
        assert!(version_less_than("1.9.0", "1.10.0"));
        assert!(!version_less_than("1.3.0", "1.3.0"), "相等不算小于");
        assert!(version_less_than("1.3", "1.3.1"), "缺位补 0");
    }

    #[test]
    fn 非法id拒绝() {
        let mut m = sample();
        m.id = "../evil".into();
        assert!(m.validate("1.3.1").is_err());
        m.id = "".into();
        assert!(m.validate("1.3.1").is_err());
        m.id = "has space".into();
        assert!(m.validate("1.3.1").is_err());
    }

    #[test]
    fn 类型平台不匹配拒绝() {
        let mut m = sample();
        m.plugin_type = "theme".into();
        assert!(m.validate("1.3.1").is_err());
        let mut m = sample();
        m.platform = vec!["linux".into()];
        assert!(m.validate("1.3.1").is_err());
    }

    #[test]
    fn 缺checksum拒绝() {
        let mut m = sample();
        m.checksum = "  ".into();
        assert!(m.validate("1.3.1").is_err());
    }

    #[test]
    fn 带bom的清单也能解析() {
        // PowerShell 5.1 的 UTF8 输出自带 BOM，必须容错
        let dir = tempfile::tempdir().unwrap();
        let m = sample();
        let json = format!("\u{FEFF}{}", serde_json::to_string(&m).unwrap());
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        let loaded = PluginManifest::load(dir.path()).expect("带 BOM 应能解析");
        assert_eq!(loaded.id, m.id);
    }

    #[test]
    fn 新字段缺省时取默认值() {
        // 老插件清单没有 category / timeout_secs，必须向后兼容
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "id": "old-plugin",
            "name": "老插件",
            "version": "1.0.0",
            "type": "tts_engine",
            "platform": ["windows"],
            "entry": "plugin.dll",
            "min_app_version": "1.0.0",
            "checksum": "abc"
        });
        std::fs::write(dir.path().join("manifest.json"), json.to_string()).unwrap();
        let m = PluginManifest::load(dir.path()).unwrap();
        assert_eq!(m.category, "remote", "缺省应为 remote");
        assert_eq!(m.timeout_secs, 60, "缺省超时应为 60 秒");
        assert!(m.requirements.is_none(), "缺省资源需求应为空");
        assert!(m.validate("1.4.0").is_ok());
    }

    #[test]
    fn 本地插件清单字段解析() {
        let dir = tempfile::tempdir().unwrap();
        let json = serde_json::json!({
            "id": "genie-tts",
            "name": "Genie TTS",
            "version": "1.0.0",
            "type": "tts_engine",
            "platform": ["windows"],
            "entry": "plugin.dll",
            "min_app_version": "1.4.0",
            "checksum": "abc",
            "category": "local",
            "timeout_secs": 1200,
            "requirements": "首次约 800MB 下载，运行时约占 2–4GB 内存"
        });
        std::fs::write(dir.path().join("manifest.json"), json.to_string()).unwrap();
        let m = PluginManifest::load(dir.path()).unwrap();
        assert_eq!(m.category, "local");
        assert_eq!(m.timeout_secs, 1200);
        assert!(m.requirements.as_deref().unwrap_or("").contains("800MB"));
    }
}
