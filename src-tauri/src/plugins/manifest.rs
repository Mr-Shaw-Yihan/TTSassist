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
    /// 插件配置声明（可选，通用插件配置机制）：声明本插件需要用户填写
    /// 的配置项（API Key 等），宿主按声明渲染通用设置面板并注入环境变量。
    /// 缺省 = 无配置项，老插件零影响。设计见 doc/通用插件配置机制设计.md。
    #[serde(default)]
    pub config: Option<PluginConfigDecl>,
}

/// 插件配置声明（manifest.config）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfigDecl {
    /// 配置获取指引链接（如开放平台控制台地址），设置面板展示
    #[serde(default)]
    pub help_url: Option<String>,
    /// 配置字段列表（空 = 无配置项）
    #[serde(default)]
    pub fields: Vec<PluginConfigField>,
}

/// 单个配置字段声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfigField {
    /// 字段标识（插件内唯一，只允许字母/数字/_/-）
    pub key: String,
    /// 字段类型：text / secret / select / number（缺省 text）
    #[serde(default = "default_field_type")]
    pub r#type: String,
    /// UI 标签
    pub label: String,
    /// 辅助说明（可选）
    #[serde(default)]
    pub description: String,
    /// 输入占位提示（可选）
    #[serde(default)]
    pub placeholder: String,
    /// 注入的环境变量名（插件用 env::var 读）
    pub env: String,
    /// 是否必填（可选，默认 false；未填只提示不阻断保存）
    #[serde(default)]
    pub required: bool,
    /// 仅 select 类型：选项列表 [{"value":"a","label":"甲"}]
    #[serde(default)]
    pub options: Option<Vec<serde_json::Value>>,
}

impl PluginConfigDecl {
    /// 声明用到的全部环境变量名（冲突检测用）
    pub fn env_names(&self) -> impl Iterator<Item = &str> {
        self.fields.iter().map(|f| f.env.as_str())
    }
}

fn default_category() -> String {
    "remote".to_string()
}

fn default_field_type() -> String {
    "text".to_string()
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
        if let Some(config) = &self.config {
            self.validate_config(config)?;
        }
        Ok(())
    }

    /// 校验配置声明：key/env 合法、type 白名单、select 必须带选项
    fn validate_config(&self, config: &PluginConfigDecl) -> Result<(), PluginError> {
        let legal = |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let mut seen_keys = std::collections::HashSet::new();
        for f in &config.fields {
            if !legal(&f.key) {
                return Err(PluginError::Unsupported(format!(
                    "插件「{}」配置字段 key「{}」不合法（只允许字母、数字、-、_，最长 64）",
                    self.id, f.key
                )));
            }
            if !legal(&f.env) {
                return Err(PluginError::Unsupported(format!(
                    "插件「{}」配置字段「{}」的 env「{}」不合法",
                    self.id, f.key, f.env
                )));
            }
            if !seen_keys.insert(f.key.clone()) {
                return Err(PluginError::Unsupported(format!(
                    "插件「{}」配置字段 key「{}」重复",
                    self.id, f.key
                )));
            }
            if !matches!(f.r#type.as_str(), "text" | "secret" | "select" | "number") {
                return Err(PluginError::Unsupported(format!(
                    "插件「{}」配置字段「{}」类型「{}」不受支持（text/secret/select/number）",
                    self.id, f.key, f.r#type
                )));
            }
            if f.r#type == "select" && f.options.as_deref().map_or(true, |o| o.is_empty()) {
                return Err(PluginError::Unsupported(format!(
                    "插件「{}」select 字段「{}」必须声明非空 options",
                    self.id, f.key
                )));
            }
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
            config: None,
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

    /// 带 config 声明的合法清单（云端插件典型样例）
    fn manifest_json_with_config(config: serde_json::Value) -> String {
        serde_json::json!({
            "id": "foo-tts",
            "name": "Foo TTS",
            "version": "0.1.0",
            "type": "tts_engine",
            "platform": ["windows"],
            "entry": "plugin.dll",
            "min_app_version": "1.0.0",
            "checksum": "abc",
            "config": config
        })
        .to_string()
    }

    #[test]
    fn 合法config声明解析并校验通过() {
        let dir = tempfile::tempdir().unwrap();
        let json = manifest_json_with_config(serde_json::json!({
            "help_url": "https://foo.example.com/keys",
            "fields": [
                { "key": "api_key", "type": "secret", "label": "API Key",
                  "env": "FOO_API_KEY", "required": true,
                  "description": "在 Foo 控制台创建" },
                { "key": "endpoint", "label": "自定义端点", "env": "FOO_ENDPOINT",
                  "placeholder": "https://api.foo.com" }
            ]
        }));
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        let m = PluginManifest::load(dir.path()).unwrap();
        let config = m.config.clone().expect("config 应解析");
        assert_eq!(config.fields.len(), 2);
        assert_eq!(config.fields[0].r#type, "secret");
        // type 缺省为 text
        assert_eq!(config.fields[1].r#type, "text");
        assert!(!config.fields[1].required);
        assert!(m.validate("1.4.0").is_ok());
        // env_names 供冲突检测
        assert_eq!(config.env_names().collect::<Vec<_>>(), vec!["FOO_API_KEY", "FOO_ENDPOINT"]);
    }

    #[test]
    fn 未知字段类型拒绝加载() {
        let dir = tempfile::tempdir().unwrap();
        let json = manifest_json_with_config(serde_json::json!({
            "fields": [ { "key": "a", "type": "color", "label": "颜色", "env": "FOO_A" } ]
        }));
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        let m = PluginManifest::load(dir.path()).unwrap();
        let err = m.validate("1.4.0").unwrap_err();
        assert!(err.to_string().contains("不受支持"), "实际: {err}");
    }

    #[test]
    fn config字段key或env非法拒绝() {
        let dir = tempfile::tempdir().unwrap();
        let json = manifest_json_with_config(serde_json::json!({
            "fields": [ { "key": "a b", "label": "x", "env": "FOO_A" } ]
        }));
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        assert!(PluginManifest::load(dir.path()).unwrap().validate("1.4.0").is_err());

        let json = manifest_json_with_config(serde_json::json!({
            "fields": [ { "key": "a", "label": "x", "env": "" } ]
        }));
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        assert!(PluginManifest::load(dir.path()).unwrap().validate("1.4.0").is_err());
    }

    #[test]
    fn select缺options拒绝() {
        let dir = tempfile::tempdir().unwrap();
        let json = manifest_json_with_config(serde_json::json!({
            "fields": [ { "key": "tier", "type": "select", "label": "档位", "env": "FOO_TIER" } ]
        }));
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        let err = PluginManifest::load(dir.path()).unwrap().validate("1.4.0").unwrap_err();
        assert!(err.to_string().contains("options"), "实际: {err}");
    }

    #[test]
    fn 带bom的config清单也能解析() {
        let dir = tempfile::tempdir().unwrap();
        let json = format!(
            "\u{FEFF}{}",
            manifest_json_with_config(serde_json::json!({
                "fields": [ { "key": "api_key", "type": "secret", "label": "K", "env": "FOO_API_KEY" } ]
            }))
        );
        std::fs::write(dir.path().join("manifest.json"), json).unwrap();
        let m = PluginManifest::load(dir.path()).expect("带 BOM 应能解析");
        assert!(m.config.unwrap().fields.len() == 1);
    }
}
