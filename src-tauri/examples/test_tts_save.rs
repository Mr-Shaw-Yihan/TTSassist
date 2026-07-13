// TTS 手动测试示例：用真实 MiMo 合成一段音频并保存到桌面。
//
// 使用方式：
//   cd src-tauri
//   set MIMO_API_KEY=你的key
//   cargo run --example test_tts_save
//
// 音频会保存到桌面，文件名 VoiceAssist_test_audio.wav

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("MIMO_API_KEY")
        .expect("请设置 MIMO_API_KEY 环境变量");

    let data_dir = std::env::temp_dir().join("voiceassist_test");
    std::fs::create_dir_all(&data_dir)?;

    let engine = voiceassist_lib::tts::mimo::MimoEngine::new(api_key, data_dir.clone());

    let rt = tokio::runtime::Runtime::new()?;
    let rel_path = rt.block_on(async {
        use voiceassist_lib::tts::traits::TTSEngine;  // ← 导入 trait
        let params = voiceassist_lib::tts::traits::TTSParams::new("你好，这是一段测试语音。欢迎使用 VoiceAssist！");
        engine.generate(params).await
    })?;

    let abs_path = data_dir.join(&rel_path);
    println!("音频已生成: {}", abs_path.display());

    // 复制到桌面
    let desktop: PathBuf = {
        if cfg!(target_os = "windows") {
            let dir = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".into());
            PathBuf::from(dir).join("Desktop")
        } else {
            let dir = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(dir).join("Desktop")
        }
    };
    let dest = desktop.join("VoiceAssist_test_audio.wav");
    std::fs::copy(&abs_path, &dest)?;
    println!("已复制到桌面: {}", dest.display());

    let meta = std::fs::metadata(&abs_path)?;
    println!("文件大小: {} 字节", meta.len());

    Ok(())
}