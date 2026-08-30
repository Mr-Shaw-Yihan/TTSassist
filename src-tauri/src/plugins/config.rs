// 通用插件配置：manifest 声明 → 环境变量注入。
//
// 插件在 manifest.json 声明所需配置项（key/env/type 等），宿主把用户在
// 设置页通用面板填写的值（settings.plugin_config）按声明注入环境变量；
// 插件侧沿用 std::env::var 读取，配置改完立即生效（下次合成即读到新值）。
// 设计见 doc/通用插件配置机制设计.md。

use std::collections::HashMap;

use super::manifest::{PluginConfigField, PluginManifest};

/// 按声明把一组值注入环境变量：非空 set_var，空则 remove_var。
/// values 缺失（插件未配置过）等价于全部为空。
pub fn inject_fields(fields: &[PluginConfigField], values: Option<&HashMap<String, String>>) {
    for f in fields {
        let v = values
            .and_then(|m| m.get(&f.key))
            .map(|s| s.trim())
            .unwrap_or("");
        if v.is_empty() {
            std::env::remove_var(&f.env);
        } else {
            std::env::set_var(&f.env, v);
        }
    }
}

/// 按 manifest 声明注入一个插件的全部配置值。
pub fn inject_manifest(manifest: &PluginManifest, values: Option<&HashMap<String, String>>) {
    if let Some(decl) = &manifest.config {
        inject_fields(&decl.fields, values);
    }
}

/// 移除一个插件声明用到的全部环境变量（卸载/清空配置时）。
pub fn remove_manifest_envs(manifest: &PluginManifest) {
    if let Some(decl) = &manifest.config {
        for env in decl.env_names() {
            std::env::remove_var(env);
        }
    }
}

/// env 名冲突检测：新加载插件与已加载插件的【必填】字段声明了同一 env 名时，
/// 返回冲突对方的插件 id。防止插件 A 的配置被插件 B 读走（可选字段撞名只提示不拦截）。
pub fn find_required_env_conflict(
    loaded: &[(String, PluginManifest)],
    new_id: &str,
    new_manifest: &PluginManifest,
) -> Option<String> {
    let Some(new_decl) = &new_manifest.config else {
        return None;
    };
    for (existing_id, m) in loaded {
        if existing_id == new_id {
            continue;
        }
        let Some(decl) = &m.config else {
            continue;
        };
        for nf in &new_decl.fields {
            if !nf.required {
                continue;
            }
            if decl.fields.iter().any(|ef| ef.required && ef.env == nf.env) {
                return Some(existing_id.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, env: &str, required: bool) -> PluginConfigField {
        PluginConfigField {
            key: key.into(),
            r#type: "secret".into(),
            label: format!("{key} label"),
            description: String::new(),
            placeholder: String::new(),
            env: env.into(),
            required,
            options: None,
        }
    }

    fn manifest_with(id: &str, fields: Vec<PluginConfigField>) -> PluginManifest {
        let mut m = super::super::manifest::PluginManifest {
            id: id.into(),
            name: id.into(),
            version: "0.1.0".into(),
            plugin_type: "tts_engine".into(),
            platform: vec!["windows".into()],
            entry: "plugin.dll".into(),
            min_app_version: "1.0.0".into(),
            checksum: "abc".into(),
            description: String::new(),
            category: "remote".into(),
            timeout_secs: 60,
            requirements: None,
            config: None,
            requires_host_bridge: false,
        };
        m.config = Some(super::super::manifest::PluginConfigDecl {
            help_url: None,
            fields,
        });
        m
    }

    // 环境变量是进程全局的，测试用独立变量名避免与并行测试/宿主进程互相干扰
    #[test]
    fn 注入_非空set空值remove() {
        std::env::remove_var("VA_TEST_INJECT_A");
        let fields = vec![field("api_key", "VA_TEST_INJECT_A", true)];

        inject_fields(&fields, None);
        assert!(std::env::var("VA_TEST_INJECT_A").is_err(), "缺省应 remove");

        let mut values = HashMap::new();
        values.insert("api_key".to_string(), " k1 ".to_string());
        inject_fields(&fields, Some(&values));
        assert_eq!(std::env::var("VA_TEST_INJECT_A").unwrap(), "k1", "应去除首尾空白");

        values.insert("api_key".to_string(), String::new());
        inject_fields(&fields, Some(&values));
        assert!(std::env::var("VA_TEST_INJECT_A").is_err(), "空值应 remove");

        std::env::remove_var("VA_TEST_INJECT_A");
    }

    #[test]
    fn 冲突检测_必填撞名返回对方可选放行() {
        let a = manifest_with(
            "plugin-a",
            vec![field("api_key", "VA_TEST_CONFLICT_K", true)],
        );
        let b = manifest_with(
            "plugin-b",
            vec![field("api_key", "VA_TEST_CONFLICT_K", true)],
        );
        let loaded = vec![("plugin-a".to_string(), a)];
        assert_eq!(
            find_required_env_conflict(&loaded, "plugin-b", &b),
            Some("plugin-a".to_string())
        );

        // 可选字段撞名：不拦截
        let c = manifest_with(
            "plugin-c",
            vec![field("api_key", "VA_TEST_CONFLICT_K", false)],
        );
        assert!(find_required_env_conflict(&loaded, "plugin-c", &c).is_none());

        // 无 config 声明：直接放行
        let mut d = manifest_with("plugin-d", vec![]);
        d.config = None;
        assert!(find_required_env_conflict(&loaded, "plugin-d", &d).is_none());
    }

    #[test]
    fn 移除声明变量() {
        std::env::set_var("VA_TEST_RM_B", "x");
        let m = manifest_with("p", vec![field("k", "VA_TEST_RM_B", false)]);
        remove_manifest_envs(&m);
        assert!(std::env::var("VA_TEST_RM_B").is_err());
    }
}
