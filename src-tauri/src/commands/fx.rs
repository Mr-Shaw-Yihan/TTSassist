// 语音效果器命令：预设清单一维合并（各已加载 audio_effect 插件的 va_fx_list 汇总）。
//
// 预设选择/排序/参数不设专门命令——前端走通用 update_setting("fx_preset"/"fx_order"/
// "fx_params") 通道（白名单 arm 已加），与设计 §五 一致。

use tauri::State;

use crate::plugins::PluginManager;

/// 一个可选预设（一维扁平模型；「原声 off」由前端固定渲染为首项，不来自插件）
#[derive(Debug, Clone, serde::Serialize)]
pub struct FxPresetInfo {
    /// 全名 "plugin_id:effect_id"（fx_preset 存的就是它）
    pub key: String,
    /// 展示名（插件给的中文短语）
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// 所属插件 id
    pub plugin: String,
    /// 所属插件展示名
    pub plugin_name: String,
    /// 插件内效果 id（ASCII 蛇形）
    pub effect_id: String,
    /// 可调参数声明（v1 仅占位，UI 不开放调节）
    pub params: Vec<plugin_api::FxParamItem>,
}

/// 已安装并加载成功的 audio_effect 插件提供的全部预设，按 key 字典序稳定输出。
#[tauri::command]
pub fn list_fx_presets(plugins: State<'_, PluginManager>) -> Vec<FxPresetInfo> {
    let mut out: Vec<FxPresetInfo> = Vec::new();
    for (plugin_id, plugin) in plugins.loaded_fx_all() {
        let json = plugin.query_effects_json();
        let effects: Vec<plugin_api::FxEffectItem> = match serde_json::from_str(&json) {
            Ok(list) => list,
            Err(_) => continue, // 清单损坏按空处理，不影响其它插件
        };
        let plugin_name = plugin.manifest.name.clone();
        for e in effects {
            out.push(FxPresetInfo {
                key: format!("{plugin_id}:{}", e.id),
                name: e.label,
                description: e.description,
                plugin: plugin_id.clone(),
                plugin_name: plugin_name.clone(),
                effect_id: e.id,
                params: e.params,
            });
        }
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}
