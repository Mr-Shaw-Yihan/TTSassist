// TTS 引擎模块入口。
// 本体已无内置 TTS 引擎：云端与本地引擎一律是插件，由 plugins::PluginManager 加载，
// 分发在 commands/tts.rs（settings.tts_engine 即插件 id）。
// 这里只剩引擎合同 traits.rs——基座不认引擎身份（见 AGENTS.md 基座合同）。

pub mod traits;
