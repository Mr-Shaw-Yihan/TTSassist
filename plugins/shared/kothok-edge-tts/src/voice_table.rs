// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Static offline voice tables, language normalization, and label formatting.
//!
//! Fallback voice lists used when the dynamic voice catalogue is unavailable.
//! [`normalize_lang`] maps partial or full language tags to their default
//! BCP-47 locale; [`voices_for_lang`] returns matching voices from the dynamic
//! catalogue or the hardcoded fallback. The raw fallback tuples live in
//! [`crate::voice_table_data`].

use crate::voice_table_data::{fallback_table, ALL_FALLBACK};
use crate::voice_types::{VoiceEntry, VoiceInfo, DYNAMIC_VOICES};

/// Default English voice short-name.
pub const DEFAULT_VOICE_EN: &str = "en-US-EmmaMultilingualNeural";

/// Default Bengali voice short-name.
pub const DEFAULT_VOICE_BN: &str = "bn-BD-NabanitaNeural";

/// Map a language tag (full BCP-47 or just a prefix like `"bn"`) to its
/// default locale. Unknown prefixes fall back to `"en-US"`.
pub fn normalize_lang(lang: &str) -> &'static str {
    let lower = lang.to_lowercase();
    let prefix = lower.split('-').next().unwrap_or(&lower);
    match prefix {
        "bn" => "bn-BD",
        "hi" => "hi-IN",
        "mr" => "mr-IN",
        "ar" => "ar-SA",
        "ur" => "ur-PK",
        "ja" => "ja-JP",
        "zh" => "zh-CN",
        "ko" => "ko-KR",
        "th" => "th-TH",
        "fr" => "fr-FR",
        "de" => "de-DE",
        "es" => "es-ES",
        "it" => "it-IT",
        "pt" => "pt-BR",
        "ru" => "ru-RU",
        "tr" => "tr-TR",
        "id" => "id-ID",
        "vi" => "vi-VN",
        "el" => "el-GR",
        "he" | "iw" => "he-IL",
        "ka" => "ka-GE",
        "hy" => "hy-AM",
        "am" => "am-ET",
        "gu" => "gu-IN",
        "pa" => "pa-IN",
        "ta" => "ta-IN",
        "te" => "te-IN",
        "kn" => "kn-IN",
        "ml" => "ml-IN",
        "si" => "si-LK",
        "lo" => "lo-LA",
        "km" => "km-KH",
        "my" => "my-MM",
        _ => "en-US",
    }
}

/// Return voices matching `lang` from the dynamic catalogue, or from the
/// hardcoded fallback tables if no dynamic voices are loaded.
pub fn voices_for_lang(lang: &str) -> Vec<VoiceEntry> {
    let normalized = normalize_lang(lang);
    let prefix = normalized.split('-').next().unwrap_or(normalized);

    if let Some(dynamic) = DYNAMIC_VOICES.get() {
        let matching: Vec<VoiceEntry> = dynamic
            .iter()
            .filter(|v| v.locale().starts_with(prefix))
            .map(|v| VoiceEntry::new(v.short_name().to_owned(), format_voice_label(v)))
            .collect();
        if !matching.is_empty() {
            return matching;
        }
    }

    fallback_voices_for_lang(normalized)
}

/// Look up a human-readable label for a voice by its short-name.
pub fn voice_label(voice: &str) -> String {
    if let Some(dynamic) = DYNAMIC_VOICES.get() {
        if let Some(v) = dynamic.iter().find(|v| v.short_name() == voice) {
            return format_voice_label(v);
        }
    }

    for table in ALL_FALLBACK {
        if let Some(&(_, label)) = table.iter().find(|(id, _)| *id == voice) {
            return label.to_string();
        }
    }
    "Default voice".to_string()
}

fn fallback_voices_for_lang(normalized: &str) -> Vec<VoiceEntry> {
    fallback_table(normalized)
        .iter()
        .map(|(id, label)| VoiceEntry::new(*id, *label))
        .collect()
}

pub(crate) fn format_voice_label(v: &VoiceInfo) -> String {
    let name = v
        .short_name()
        .splitn(3, '-')
        .nth(2)
        .map(|s| s.strip_suffix("Neural").unwrap_or(s))
        .map(|s| s.strip_suffix("Multilingual").unwrap_or(s))
        .unwrap_or("Voice");
    format!("{name} ({})", v.locale())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_known_prefixes() {
        assert_eq!(normalize_lang("bn"), "bn-BD");
        assert_eq!(normalize_lang("ar"), "ar-SA");
        assert_eq!(normalize_lang("ja"), "ja-JP");
        assert_eq!(normalize_lang("en"), "en-US");
    }

    #[test]
    fn normalize_extended_script_prefixes() {
        assert_eq!(normalize_lang("el"), "el-GR");
        assert_eq!(normalize_lang("ru"), "ru-RU");
        assert_eq!(normalize_lang("he"), "he-IL");
        assert_eq!(normalize_lang("iw"), "he-IL");
        assert_eq!(normalize_lang("ka"), "ka-GE");
        assert_eq!(normalize_lang("hy"), "hy-AM");
        assert_eq!(normalize_lang("am"), "am-ET");
        assert_eq!(normalize_lang("gu"), "gu-IN");
        assert_eq!(normalize_lang("pa"), "pa-IN");
        assert_eq!(normalize_lang("ta"), "ta-IN");
        assert_eq!(normalize_lang("te"), "te-IN");
        assert_eq!(normalize_lang("kn"), "kn-IN");
        assert_eq!(normalize_lang("ml"), "ml-IN");
        assert_eq!(normalize_lang("si"), "si-LK");
        assert_eq!(normalize_lang("lo"), "lo-LA");
        assert_eq!(normalize_lang("km"), "km-KH");
        assert_eq!(normalize_lang("my"), "my-MM");
        assert_eq!(normalize_lang("ko"), "ko-KR");
        assert_eq!(normalize_lang("zh"), "zh-CN");
    }

    #[test]
    fn normalize_full_tag_passthrough() {
        assert_eq!(normalize_lang("bn-BD"), "bn-BD");
        assert_eq!(normalize_lang("hi-IN"), "hi-IN");
    }

    #[test]
    fn normalize_unknown_falls_back_to_en() {
        assert_eq!(normalize_lang("zz"), "en-US");
        assert_eq!(normalize_lang(""), "en-US");
    }

    #[test]
    fn normalize_case_insensitive() {
        assert_eq!(normalize_lang("BN"), "bn-BD");
        assert_eq!(normalize_lang("Ja-JP"), "ja-JP");
    }

    #[test]
    fn fallback_table_returns_tables() {
        assert!(!fallback_table("bn-BD").is_empty());
        assert!(!fallback_table("en-US").is_empty());
        assert!(!fallback_table("ar-SA").is_empty());
    }

    #[test]
    fn fallback_table_covers_every_loadable_script() {
        for loc in [
            "el-GR", "ru-RU", "he-IL", "ka-GE", "hy-AM", "am-ET", "gu-IN", "pa-IN", "ta-IN",
            "te-IN", "kn-IN", "ml-IN", "si-LK", "lo-LA", "km-KH", "my-MM", "ko-KR", "zh-CN",
        ] {
            let table = fallback_table(loc);
            assert!(
                !table.is_empty() && table != fallback_table("en-US"),
                "{loc} must resolve to a dedicated voice table, not the English fallback"
            );
        }
    }

    #[test]
    fn fallback_voices_unknown_lang_returns_en() {
        let unknown = fallback_voices_for_lang("zz-ZZ");
        let en = fallback_voices_for_lang("en-US");
        assert_eq!(unknown, en);
    }

    #[test]
    fn format_label_extracts_name() {
        let v = VoiceInfo::new(
            "en-US-EmmaMultilingualNeural".into(),
            "en-US".into(),
            "Female".into(),
            "Emma".into(),
        );
        assert_eq!(format_voice_label(&v), "Emma (en-US)");
    }

    #[test]
    fn format_label_strips_neural_suffix() {
        let v = VoiceInfo::new(
            "ja-JP-NanamiNeural".into(),
            "ja-JP".into(),
            "Female".into(),
            "Nanami".into(),
        );
        assert_eq!(format_voice_label(&v), "Nanami (ja-JP)");
    }

    #[test]
    fn format_label_unknown_format() {
        let v = VoiceInfo::new("x-Y-Z".into(), "x-Y".into(), "Male".into(), "Z".into());
        assert_eq!(format_voice_label(&v), "Z (x-Y)");
    }

    #[test]
    fn voice_label_finds_fallback_entry() {
        assert_eq!(voice_label("en-US-EmmaMultilingualNeural"), "Emma (US)");
        assert_eq!(voice_label("bn-BD-NabanitaNeural"), "Nabanita (BD)");
    }

    #[test]
    fn voice_label_unknown_returns_default() {
        assert_eq!(voice_label("nonexistent-Voice"), "Default voice");
    }

    #[test]
    fn default_voice_consts() {
        assert!(DEFAULT_VOICE_EN.starts_with("en-US-"));
        assert!(DEFAULT_VOICE_BN.starts_with("bn-BD-"));
    }

    #[test]
    fn voices_for_lang_fallback_builds_owned_entries() {
        let voices = voices_for_lang("bn-BD");
        assert!(voices.iter().any(|v| v.id() == "bn-BD-NabanitaNeural"));
        assert!(voices.iter().all(|v| !v.label().is_empty()));
    }
}
