// 测试桩插件：验证宿主插件加载框架（libloading + C ABI + 内存约定）而存在。
// 不做真实 TTS，合成结果是确定性假数据；不打包、不发布。

plugin_api::va_tts_plugin! {
    id: "test-plugin",
    name: "测试插件",
    version: "0.1.0",
    audio_format: "wav",
    voices_json: r#"[{"id":"voice-a","label":"音色A"},{"id":"voice-b","label":"音色B"}]"#,
    synthesize: synthesize,
}

/// 假合成：文本为空报错；否则返回 "FAKE_AUDIO|文本|音色" 的字节
fn synthesize(text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
    if text.is_empty() {
        return Err("文本不能为空".to_string());
    }
    Ok(format!("FAKE_AUDIO|{}|{}", text, voice.unwrap_or("default")).into_bytes())
}
