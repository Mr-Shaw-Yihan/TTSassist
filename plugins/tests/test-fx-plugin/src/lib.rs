// 效果器测试桩插件：验证宿主 audio_effect 加载框架（libloading + C ABI + 内存约定）。
// 不做真实 DSP，处理结果是确定性假数据；不打包、不发布。

plugin_api::va_fx_plugin! {
    id: "test-fx-plugin",
    name: "效果器测试桩",
    version: "0.1.0",
    effects_json: r#"[
        {"id":"gain_half","label":"半音量","description":"乘 0.5 的确定性假效果","params":[]},
        {"id":"error_effect","label":"必报错","description":"始终返回错误，测错误链路","params":[]}
    ]"#,
    process: process,
}

fn process(input: &plugin_api::FxProcessInput) -> Result<plugin_api::FxProcessOutput, String> {
    match input.effect_id {
        "gain_half" => Ok(plugin_api::FxProcessOutput {
            pcm: input.pcm.iter().map(|s| s * 0.5).collect(),
            frames: input.frames,
            channels: input.channels,
        }),
        "error_effect" => Err("测试桩主动报错".to_string()),
        other => Err(format!("未知效果「{other}」")),
    }
}
