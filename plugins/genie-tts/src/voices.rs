// 动态音色表：扫描数据目录 characters/ 下的音色包 + 内置预置角色清单。
//
// 音色包布局（与官方预置角色一致，社区 GPT-SoVITS ONNX 模型可直接放入）：
//   characters/<音色id>/
//     ├── tts_models/        # ONNX 模型文件（convert_to_onnx 产物）
//     ├── prompt_wav/xx.wav  # 参考音频（GPT-SoVITS 零样本必需）
//     ├── prompt_wav.json    # {"Normal": {"text": "参考音频文本", "wav": "xx.wav"}}
//     └── meta.json          # 可选：{"label": "展示名", "language": "Chinese"}
//
// 该函数作为 va_tts_plugin! 宏的 voices 参数，宿主每次查音色表都会调用，
// 因此保持纯磁盘扫描、不触发网络与服务进程。

use plugin_api::VoiceItem;

/// 预置角色（官方 High-Logic/Genie 提供，首次使用时服务端自动下载）
struct Predefined {
    id: &'static str,
    label: &'static str,
}

const PREDEFINED: &[Predefined] = &[
    Predefined { id: "feibi", label: "菲比（鸣潮·中文）" },
    Predefined { id: "mika", label: "聖園ミカ（蔚蓝档案·日文）" },
    Predefined { id: "thirtyseven", label: "37（重返未来1999·英文）" },
];

/// 默认音色（中文预置角色）
pub const DEFAULT_VOICE: &str = "feibi";

/// 生成当前音色表：已安装音色包 + 未安装的预置角色（标注待下载）
pub fn list_voices() -> Vec<VoiceItem> {
    let mut voices = Vec::new();

    // 1. 已安装的音色包（characters/ 子目录，布局完整才算）
    let installed = installed_packs();

    // 预置角色：已装用包内 meta 名，未装标注"首次使用自动下载"
    for p in PREDEFINED {
        match installed.iter().find(|(id, _label)| id == p.id) {
            Some((id, label)) => voices.push(VoiceItem {
                id: id.clone(),
                label: label.clone(),
            }),
            None => voices.push(VoiceItem {
                id: p.id.to_string(),
                label: format!("{} · 首次使用自动下载", p.label),
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

/// 扫描 characters/ 下布局完整的音色包，返回 (目录名, 展示名)
fn installed_packs() -> Vec<(String, String)> {
    let ctx = match crate::paths::Ctx::get() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let root = ctx.characters_dir();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut packs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // 布局校验：tts_models/ + prompt_wav.json
        if !path.join("tts_models").is_dir() || !path.join("prompt_wav.json").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let label = read_meta_label(&path).unwrap_or_else(|| id.clone());
        packs.push((id, label));
    }
    // 稳定顺序（按目录名）
    packs.sort_by(|a, b| a.0.cmp(&b.0));
    packs
}

/// 读 meta.json 的 label（缺失/损坏返回 None）
fn read_meta_label(pack_dir: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(pack_dir.join("meta.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(raw.trim_start_matches('\u{FEFF}')).ok()?;
    v.get("label")?.as_str().map(|s| s.to_string())
}
