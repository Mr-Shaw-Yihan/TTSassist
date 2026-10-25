// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Prosody rate string mapping and validation.

/// Baseline offset (percentage points) applied to every prosody rate.
///
/// Edge-TTS normal speed feels too fast for e-reader listening, so every speed
/// is shifted slower by this many percentage points. The slider still spans
/// the full -100%..+100% range around it.
pub const RATE_BASELINE_OFFSET: i32 = -10;

/// Map a 0..100 slider value to an Edge-TTS SSML prosody rate percentage.
pub fn rate_percent(speed: i32) -> i32 {
    ((speed - 50) * 2 + RATE_BASELINE_OFFSET).clamp(-100, 100)
}

/// Format a 0..100 slider value as an Edge-TTS rate string, e.g. "-10%".
pub fn rate_string(speed: i32) -> String {
    let pct = rate_percent(speed);
    let sign = if pct >= 0 { "+" } else { "" };
    format!("{sign}{pct}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midpoint_is_baseline_offset() {
        assert_eq!(rate_percent(50), RATE_BASELINE_OFFSET);
    }

    #[test]
    fn max_speed_is_offset_from_100() {
        assert_eq!(rate_percent(100), 90);
    }

    #[test]
    fn min_speed_clamps_to_neg_100() {
        assert_eq!(rate_percent(0), -100);
    }

    #[test]
    fn rate_string_positive_has_plus() {
        assert_eq!(rate_string(100), "+90%");
    }

    #[test]
    fn rate_string_negative_has_no_plus() {
        let s = rate_string(0);
        assert_eq!(s, "-100%");
    }

    #[test]
    fn rate_string_midpoint_includes_offset() {
        let s = rate_string(50);
        assert!(s.contains("-10%"));
    }
}
