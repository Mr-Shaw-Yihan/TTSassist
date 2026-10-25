// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! SSML document construction and text sanitization for Edge-TTS.

const NEUTRAL_PITCH: &str = "+0Hz";
const NEUTRAL_VOLUME: &str = "+0%";

/// Replace XML-incompatible control characters (0x00-0x08, 0x0B-0x0C,
/// 0x0E-0x1F) with spaces.  The Edge endpoint rejects raw control chars
/// in the SSML body.
fn sanitize_control_chars(text: &str) -> String {
    text.chars()
        .map(|c| {
            let code = c as u32;
            if code <= 8 || (11..=12).contains(&code) || (14..=31).contains(&code) {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// XML-escape the five special characters for safe embedding in SSML.
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&apos;")
        .replace('"', "&quot;")
}

/// Build the SSML `<speak>` document for a single utterance.
///
/// `pitch` and `volume` are fixed at neutral (`+0Hz` / `+0%`); only `rate` is
/// caller-controlled.
pub(crate) fn build_ssml(text: &str, voice: &str, rate: &str, lang: &str) -> String {
    let clean = sanitize_control_chars(text);
    let escaped = xml_escape(&clean);
    let voice = xml_escape(voice);
    let lang = xml_escape(lang);
    let rate = xml_escape(rate);
    format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='{lang}'>\
         <voice name='{voice}'>\
         <prosody pitch='{NEUTRAL_PITCH}' rate='{rate}' volume='{NEUTRAL_VOLUME}'>\
         {escaped}\
         </prosody></voice></speak>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_basic() {
        assert_eq!(
            xml_escape("a<b>c&d'e\"f"),
            "a&lt;b&gt;c&amp;d&apos;e&quot;f"
        );
    }

    #[test]
    fn sanitize_strips_control_chars() {
        let cleaned = sanitize_control_chars("a\x01b\x07c");
        assert_eq!(cleaned, "a b c");
    }

    #[test]
    fn sanitize_preserves_newlines() {
        assert_eq!(sanitize_control_chars("a\nb"), "a\nb");
    }

    #[test]
    fn build_ssml_contains_voice_and_rate() {
        let ssml = build_ssml("hi", "en-US-EmmaMultilingualNeural", "+25%", "en-US");
        assert!(ssml.contains("name='en-US-EmmaMultilingualNeural'"));
        assert!(ssml.contains("rate='+25%'"));
        assert!(ssml.contains("xml:lang='en-US'"));
        assert!(ssml.contains(">hi<"));
    }

    #[test]
    fn xml_escape_empty_string() {
        assert_eq!(xml_escape(""), "");
    }

    #[test]
    fn xml_escape_no_special_chars() {
        assert_eq!(xml_escape("hello world"), "hello world");
    }

    #[test]
    fn sanitize_preserves_tab_and_cr() {
        assert_eq!(sanitize_control_chars("a\tb"), "a\tb");
        assert_eq!(sanitize_control_chars("a\rb"), "a\rb");
    }

    #[test]
    fn sanitize_all_control_chars_replaced() {
        let input: String = (0u8..=31).map(|b| b as char).collect();
        let cleaned = sanitize_control_chars(&input);
        let spaces = cleaned.chars().filter(|&c| c == ' ').count();
        let control_chars = 9 + 2 + 18; // 0x00-0x08, 0x0B-0x0C, 0x0E-0x1F
        assert_eq!(spaces, control_chars);
    }

    #[test]
    fn build_ssml_escapes_special_chars() {
        let ssml = build_ssml("a<b>&c", "v", "+0%", "en");
        assert!(ssml.contains("&lt;"));
        assert!(ssml.contains("&gt;"));
        assert!(ssml.contains("&amp;"));
        assert!(!ssml.contains("a<b>&c"));
    }

    #[test]
    fn build_ssml_escapes_attribute_values() {
        let ssml = build_ssml("hi", "v'oice", "+25%\"", "en'US");
        assert!(ssml.contains("name='v&apos;oice'"));
        assert!(ssml.contains("rate='+25%&quot;'"));
        assert!(ssml.contains("xml:lang='en&apos;US'"));
        assert!(!ssml.contains("v'oice"));
        assert!(!ssml.contains("en'US"));
    }

    #[test]
    fn build_ssml_empty_text() {
        let ssml = build_ssml("", "v", "+0%", "en");
        assert!(ssml.contains("></prosody>"));
    }
}
