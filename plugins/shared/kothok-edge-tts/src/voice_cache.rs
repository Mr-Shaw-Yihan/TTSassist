// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Disk-cache persistence for the voice catalogue.

use crate::voice_types::VoiceInfo;

/// Load voice data from a JSON cache file. Returns an empty vec on any
/// read/parse failure.
pub fn load_voice_cache(path: &str) -> Vec<VoiceInfo> {
    let result = std::fs::read_to_string(path).and_then(|s| {
        serde_json::from_str(&s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    });
    match result {
        Ok(voices) => voices,
        Err(e) => {
            log::debug!("voice-cache: load failed, starting fresh: {e}");
            Vec::new()
        }
    }
}

/// Write voice data to a JSON cache file.
///
/// Silently discards write failures - the cache is an optimisation, not
/// critical data.
pub fn save_voice_cache(path: &str, voices: &[VoiceInfo]) {
    if let Ok(json) = serde_json::to_string(voices) {
        // best-effort: cache is an optimisation, failure is non-critical
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_file_returns_empty() {
        let voices = load_voice_cache("/nonexistent/path/voices.json");
        assert!(voices.is_empty());
    }

    #[test]
    fn roundtrip_cache() {
        let dir = std::env::temp_dir().join("kothok_edge_tts_test_cache");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("voices.json");
        let path_str = path.to_str().unwrap();

        let voices = vec![VoiceInfo::new(
            "en-US-EmmaMultilingualNeural".into(),
            "en-US".into(),
            "Female".into(),
            "Emma".into(),
        )];
        save_voice_cache(path_str, &voices);
        let loaded = load_voice_cache(path_str);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].short_name(), "en-US-EmmaMultilingualNeural");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_invalid_path_does_not_panic() {
        save_voice_cache("/nonexistent/dir/file.json", &[]);
    }
}
