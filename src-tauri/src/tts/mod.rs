// TTS 引擎模块入口。
// 内置引擎只保留 Moss（唯一尚未插件化的云端引擎）；其余云端引擎均已抽出为插件，
// 由 plugins::PluginManager 加载。内置与插件引擎的分发都在 commands/tts.rs，
// 这里不再有统一的引擎工厂函数（基座不认引擎身份，见 AGENTS.md 基座合同）。

pub mod traits;
pub mod moss;
