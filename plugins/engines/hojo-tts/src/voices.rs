// 动态音色表：扫描数据目录 voices/ 下的音色包 + 内置预置角色清单。
//
// Hojo 的音色 = 参考音频 + 参考音频文本（零样本克隆），缺参考文本的预置
// 音色没有意义，因此预置角色全部带参考文本。音色包布局：
//   voices/<音色id>/
//     ├── ref.wav     # 参考音频（建议 5~10 秒、干净人声）
//     └── voice.json  # {"label": "展示名", "text": "参考音频逐字文本"}
//
// 预置角色 = 上游官方 demo 参考音频（assets/audio/，首次使用时从 GitHub 下载，
// 几百 KB）。参考文本来源：
// - female-zh-1（zh1.wav）：上游 README 公开的示例文本；
// - 其余四个：上游未公开，文本由 faster-whisper（small）转写并用 zh1 校准
//   （校准样本逐字命中）后采用，已在 voice.json 标记 text_source=asr。
// 性别标注依据基频实测（F0 中位数）：zh1=277Hz、female_zh_89=247Hz、
// female_zh_95=233Hz、female_en_139=203Hz 均为女声；male_en_88=122Hz 为
// 男声——上游 README 把 zh1 标成 "Code-switching Male Voice" 与实测不符，
// 以实测为准。
//
// 该函数作为 va_tts_plugin! 宏的 voices 参数，宿主每次查音色表都会调用，
// 因此保持纯磁盘扫描、不触发网络与服务进程。

use plugin_api::VoiceItem;

/// 预置角色（上游官方 demo 参考音频）
pub(crate) struct Predefined {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    /// 上游仓库 assets/audio/ 内的文件名
    pub(crate) asset: &'static str,
    /// 参考音频文本
    pub(crate) text: &'static str,
    /// 文本来源（写入 voice.json 备查）：official = 上游公开；asr = 转写
    pub(crate) text_source: &'static str,
}

const PREDEFINED: &[Predefined] = &[
    Predefined {
        id: "female-zh-1",
        label: "女声·中文一",
        asset: "zh1.wav",
        text: "现在的外卖确实坑多，要不咱换家稍微贵点的？可能品质好点。",
        text_source: "official",
    },
    Predefined {
        id: "female-zh-2",
        label: "女声·中文二",
        asset: "female_zh_89.wav",
        text: "你是不是还在为之前的事情难过，把我当成了发泄对象？如果你现在还不想说，我也不勉强你，等你心情好点了再告诉我吧。",
        text_source: "asr",
    },
    Predefined {
        id: "female-zh-3",
        label: "女声·中文三",
        asset: "female_zh_95.wav",
        text: "生活就像一盒巧克力，你永远不知道下一块会是什么味道。",
        text_source: "asr",
    },
    Predefined {
        id: "female-en",
        label: "女声·英文",
        asset: "female_en_139.wav",
        text: "The serenity of a quiet boat ride on the River Thames offers a peaceful escape from the city's hustle and bustle.",
        text_source: "asr",
    },
    Predefined {
        id: "male-en",
        label: "男声·英文",
        asset: "male_en_88.wav",
        text: "Could you explain how the stock exchange functions in the economy?",
        text_source: "asr",
    },
];

/// 上游资产下载源（raw.githubusercontent 优先，jsdelivr CDN 兜底）
const ASSET_URL_BASES: &[&str] = &[
    "https://raw.githubusercontent.com/HojoAI/Hojo-TTS-Light/main/Hojo-TTS-Light-80M/assets/audio",
    "https://cdn.jsdelivr.net/gh/HojoAI/Hojo-TTS-Light@main/Hojo-TTS-Light-80M/assets/audio",
];

/// 默认音色（参考文本来自上游官方，质量最稳）
pub const DEFAULT_VOICE: &str = "female-zh-1";

/// 预置角色的候选下载 URL 列表
pub fn predefined_asset_urls(asset: &str) -> Vec<String> {
    ASSET_URL_BASES
        .iter()
        .map(|base| format!("{base}/{asset}"))
        .collect()
}

/// 按 id 查预置角色定义
pub fn predefined(id: &str) -> Option<&'static Predefined> {
    PREDEFINED.iter().find(|p| p.id == id)
}

/// 生成当前音色表：已安装音色包 + 未安装的预置角色（标注待下载）
pub fn list_voices() -> Vec<VoiceItem> {
    let mut voices = Vec::new();

    // 1. 已安装的音色包（voices/ 子目录，布局完整才算）
    let installed = installed_packs();

    // 预置角色：已装用包内 voice.json 名，未装标注"待下载"（安装走显式流程）
    for p in PREDEFINED {
        match installed.iter().find(|(id, _label)| id == p.id) {
            Some((id, label)) => voices.push(VoiceItem {
                id: id.clone(),
                label: label.clone(),
            }),
            None => voices.push(VoiceItem {
                id: p.id.to_string(),
                label: format!("{} · 待下载", p.label),
            }),
        }
    }

    // 2. 用户自备音色包（排除已列出的预置角色）
    for (id, label) in installed {
        if !PREDEFINED.iter().any(|p| p.id == id) {
            voices.push(VoiceItem { id, label });
        }
    }

    voices
}

/// 已安装音色包的 id 列表（供 setup 状态探测用）
pub fn installed_pack_ids() -> Vec<String> {
    installed_packs().into_iter().map(|(id, _label)| id).collect()
}

/// 扫描 voices/ 下布局完整的音色包，返回 (目录名, 展示名)
fn installed_packs() -> Vec<(String, String)> {
    let ctx = match crate::paths::Ctx::get() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let root = ctx.voices_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut packs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 布局校验：ref.wav + voice.json
        if !path.join("ref.wav").is_file() || !path.join("voice.json").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let label = read_pack_label(&path).unwrap_or_else(|| id.clone());
        packs.push((id, label));
    }
    // 稳定顺序（按目录名）
    packs.sort_by(|a, b| a.0.cmp(&b.0));
    packs
}

/// 读 voice.json 的 label（缺失/损坏返回 None）
fn read_pack_label(pack_dir: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(pack_dir.join("voice.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{FEFF}')).ok()?;
    v.get("label")?.as_str().map(|s| s.to_string())
}

/// 音色 id → 展示名（预置角色用内置名，已装包读 voice.json，兜底原 id）。
/// 用于面向用户的文案，避免暴露内部 id。
pub fn display_label(voice_id: &str) -> String {
    if let Some(p) = predefined(voice_id) {
        return p.label.to_string();
    }
    if let Ok(ctx) = crate::paths::Ctx::get() {
        if let Some(label) = read_pack_label(&ctx.voices_dir().join(voice_id)) {
            return label;
        }
    }
    voice_id.to_string()
}

/// 音色 id 合法性（防路径穿越）：仅字母/数字/下划线/连字符，非空且不超过 64 字符
pub fn is_valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// 音色包目录布局校验（ref.wav + voice.json 为最低要求）
pub fn validate_pack_layout(dir: &std::path::Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("所选路径不是目录".to_string());
    }
    if !dir.join("ref.wav").is_file() {
        return Err("目录里缺少 ref.wav（参考音频），不是完整的音色包".to_string());
    }
    if !dir.join("voice.json").is_file() {
        return Err("目录里缺少 voice.json（音色信息），不是完整的音色包".to_string());
    }
    // voice.json 必须可解析
    let raw = std::fs::read_to_string(dir.join("voice.json"))
        .map_err(|e| format!("读取 voice.json 失败: {e}"))?;
    serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{FEFF}'))
        .map_err(|e| format!("voice.json 不是合法 JSON: {e}"))?;
    Ok(())
}
