// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Shared voice types and the global dynamic-voice state.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// One voice entry from the Edge voice-list JSON response.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VoiceInfo {
    #[serde(rename = "ShortName")]
    short_name: String,
    #[serde(rename = "Locale")]
    locale: String,
    #[serde(rename = "Gender")]
    gender: String,
    #[serde(rename = "FriendlyName")]
    friendly_name: String,
}

/// A voice with a short ID and a human-readable label for UI display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceEntry {
    id: String,
    label: String,
}

/// Global dynamic voice catalogue, populated after a successful fetch.
pub(crate) static DYNAMIC_VOICES: OnceLock<Vec<VoiceInfo>> = OnceLock::new();

impl VoiceInfo {
    #[cfg(test)]
    pub(crate) fn new(
        short_name: String,
        locale: String,
        gender: String,
        friendly_name: String,
    ) -> Self {
        Self {
            short_name,
            locale,
            gender,
            friendly_name,
        }
    }

    pub fn short_name(&self) -> &str {
        &self.short_name
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub fn gender(&self) -> &str {
        &self.gender
    }

    pub fn friendly_name(&self) -> &str {
        &self.friendly_name
    }
}

impl VoiceEntry {
    pub(crate) fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

/// Store the dynamic voice catalogue.  No-op if already set.
///
/// Called after a successful [`voice_fetch::list_voices`] fetch.
pub fn set_dynamic_voices(voices: Vec<VoiceInfo>) {
    // best-effort: idempotent - if already set by a concurrent caller, keep existing
    let _ = DYNAMIC_VOICES.set(voices);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_voice_info() {
        let json = r#"{
            "Name": "Microsoft Server Speech Text to Speech Voice (en-US, EmmaMultilingual)",
            "ShortName": "en-US-EmmaMultilingualNeural",
            "Locale": "en-US",
            "Gender": "Female",
            "FriendlyName": "Microsoft Emma Multilingual Online (Natural)",
            "Status": "GA"
        }"#;
        let v: VoiceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(v.short_name(), "en-US-EmmaMultilingualNeural");
        assert_eq!(v.locale(), "en-US");
        assert_eq!(v.gender(), "Female");
    }

    #[test]
    fn deserialize_voice_array() {
        let json = r#"[
            {"ShortName":"en-US-EmmaMultilingualNeural","Locale":"en-US","Gender":"Female","FriendlyName":"Emma"},
            {"ShortName":"bn-BD-NabanitaNeural","Locale":"bn-BD","Gender":"Female","FriendlyName":"Nabanita"}
        ]"#;
        let voices: Vec<VoiceInfo> = serde_json::from_str(json).unwrap();
        assert_eq!(voices.len(), 2);
        assert_eq!(voices.get(1).map(|v| v.locale()), Some("bn-BD"));
    }

    #[test]
    fn deserialize_voice_with_extra_fields() {
        let json = r#"{
            "ShortName":"ja-JP-NanamiNeural",
            "Locale":"ja-JP",
            "Gender":"Female",
            "FriendlyName":"Nanami",
            "VoiceTag":{"ContentCategories":["General"],"VoicePersonalities":["Friendly"]}
        }"#;
        let v: VoiceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(v.short_name(), "ja-JP-NanamiNeural");
    }

    #[test]
    fn voice_entry_accessors() {
        let e = VoiceEntry::new("en-US-EmmaMultilingualNeural", "Emma (US)");
        assert_eq!(e.id(), "en-US-EmmaMultilingualNeural");
        assert_eq!(e.label(), "Emma (US)");
    }

    #[test]
    fn set_dynamic_voices_idempotent() {
        let voices = vec![VoiceInfo::new(
            "x-Y-VoiceNeural".into(),
            "x-Y".into(),
            "Female".into(),
            "Voice".into(),
        )];
        set_dynamic_voices(voices);
        set_dynamic_voices(Vec::new());
        let stored = DYNAMIC_VOICES.get().unwrap();
        assert_eq!(stored.len(), 1);
    }
}
