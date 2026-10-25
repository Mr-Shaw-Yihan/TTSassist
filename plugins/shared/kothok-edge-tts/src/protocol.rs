// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Wire-protocol parsing: binary audio-frame extraction, text-message header
//! splitting, and `audio.metadata` JSON deserialization.

use crate::event::TtsEvent;
use serde::Deserialize;
use std::collections::HashMap;

/// Substring in the binary frame header that marks an audio-data payload.
const PATH_AUDIO_HEADER: &[u8] = b"Path:audio";
const WORDBOUNDARY_TYPE: &str = "WordBoundary";
pub(crate) const LOG_BODY_MAX_CHARS: usize = 160;

const LINE_ENDING: &str = "\r\n";
pub(crate) const HEADER_BODY_SEPARATOR: &str = "\r\n\r\n";

#[derive(Deserialize)]
struct AudioMetadata {
    #[serde(rename = "Metadata")]
    metadata: Vec<MetaItem>,
}

#[derive(Deserialize)]
struct MetaItem {
    #[serde(rename = "Type")]
    item_type: String,
    #[serde(rename = "Data")]
    data: MetaData,
}

#[derive(Deserialize)]
struct MetaData {
    #[serde(rename = "Offset")]
    offset: u64,
    #[serde(rename = "Duration")]
    duration: u64,
    #[serde(rename = "text")]
    text: MetaText,
}

#[derive(Deserialize)]
struct MetaText {
    #[serde(rename = "Text")]
    text: String,
}

/// Extract the audio payload from a binary WebSocket frame, if it is an
/// `audio` frame (header contains `Path:audio`).
///
/// Returns `None` for non-audio binary frames, truncated frames, or frames
/// whose 2-byte length prefix is larger than the payload.
pub(crate) fn parse_binary_audio(bin: &[u8]) -> Option<Vec<u8>> {
    let b0 = *bin.first()?;
    let b1 = *bin.get(1)?;
    let header_len = u16::from_be_bytes([b0, b1]) as usize;

    let header_end = 2usize.checked_add(header_len)?;
    let header = bin.get(2..header_end)?;
    let data = bin.get(header_end..)?;

    let is_audio = header
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .any(|line| line == PATH_AUDIO_HEADER);

    if is_audio {
        Some(data.to_vec())
    } else {
        None
    }
}

/// Split a text protocol message into a header map and a body slice.
///
/// Headers are `Key: Value` lines separated by `\r\n`; the body starts after
/// the first blank `\r\n\r\n` separator.
pub(crate) fn split_msg(text: &str) -> (HashMap<String, String>, &str) {
    let mut map = HashMap::new();
    let body = match text.find(HEADER_BODY_SEPARATOR) {
        Some(sep) => {
            for line in text.get(..sep).unwrap_or("").split(LINE_ENDING) {
                if let Some((key, val)) = line.split_once(':') {
                    map.insert(key.trim().to_string(), val.trim().to_string());
                }
            }
            text.get(sep + HEADER_BODY_SEPARATOR.len()..).unwrap_or("")
        }
        None => "",
    };
    (map, body)
}

/// Parse an `audio.metadata` JSON body into word-boundary events.
///
/// Non-`WordBoundary` entries are silently skipped.  Malformed JSON is logged
/// at `warn` and yields an empty vec - the audio stream is unaffected.
pub(crate) fn parse_word_boundaries(body: &str) -> Vec<TtsEvent> {
    let Ok(md) = serde_json::from_str::<AudioMetadata>(body) else {
        log::warn!(
            "edge-tts: unparseable audio.metadata body: {}",
            body.chars().take(LOG_BODY_MAX_CHARS).collect::<String>()
        );
        return Vec::new();
    };
    md.metadata
        .into_iter()
        .filter(|item| item.item_type == WORDBOUNDARY_TYPE)
        .map(|item| TtsEvent::WordBoundary {
            offset: item.data.offset,
            duration: item.data.duration,
            text: item.data.text.text,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio_frame(header: &str, data: &[u8]) -> Vec<u8> {
        let hl = header.len() as u16;
        let mut out = hl.to_be_bytes().to_vec();
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn parse_audio_frame_extracts_payload() {
        let frame = audio_frame("Path:audio\r\n", &[0xFF, 0xFB, 0x90]);
        let audio = parse_binary_audio(&frame).unwrap();
        assert_eq!(audio, &[0xFF, 0xFB, 0x90]);
    }

    #[test]
    fn parse_non_audio_frame_returns_none() {
        let frame = audio_frame("Path:turn.start\r\n", b"{}");
        assert!(parse_binary_audio(&frame).is_none());
    }

    #[test]
    fn parse_truncated_frame_returns_none() {
        assert!(parse_binary_audio(&[]).is_none());
        assert!(parse_binary_audio(&[0x00]).is_none());
        assert!(parse_binary_audio(&[0x00, 0xFF, b'x']).is_none());
    }

    #[test]
    fn split_msg_parses_headers_and_body() {
        let msg = "Path:ssml\r\nX-RequestId:abc\r\n\r\n<speak>hi</speak>";
        let (headers, body) = split_msg(msg);
        assert_eq!(headers.get("Path"), Some(&"ssml".to_string()));
        assert_eq!(headers.get("X-RequestId"), Some(&"abc".to_string()));
        assert_eq!(body, "<speak>hi</speak>");
    }

    #[test]
    fn split_msg_no_separator() {
        let (headers, body) = split_msg("no headers here");
        assert!(headers.is_empty());
        assert_eq!(body, "");
    }

    #[test]
    fn parse_word_boundaries_extracts_offsets() {
        let body = r#"{"Metadata":[{"Type":"WordBoundary","Data":{"Offset":1000,"Duration":5000,"text":{"Text":"hello"}}}]}"#;
        let events = parse_word_boundaries(body);
        assert_eq!(events.len(), 1);
        match &events[0] {
            TtsEvent::WordBoundary {
                offset,
                duration,
                text,
            } => {
                assert_eq!(*offset, 1000);
                assert_eq!(*duration, 5000);
                assert_eq!(text, "hello");
            }
            _ => panic!("expected WordBoundary"),
        }
    }

    #[test]
    fn parse_word_boundaries_skips_non_wordboundary() {
        let body = r#"{"Metadata":[{"Type":"SentenceBoundary","Data":{"Offset":0,"Duration":0,"text":{"Text":"x"}}}]}"#;
        assert!(parse_word_boundaries(body).is_empty());
    }

    #[test]
    fn parse_word_boundaries_malformed_json() {
        assert!(parse_word_boundaries("not json").is_empty());
    }

    #[test]
    fn parse_audio_frame_empty_header() {
        let frame = audio_frame("", &[0x01, 0x02]);
        assert!(parse_binary_audio(&frame).is_none());
    }

    #[test]
    fn parse_audio_frame_path_not_first_header() {
        let frame = audio_frame("Content-Type:audio\r\nPath:audio\r\n", &[0xDE, 0xAD]);
        let audio = parse_binary_audio(&frame).unwrap();
        assert_eq!(audio, &[0xDE, 0xAD]);
    }

    #[test]
    fn split_msg_header_value_with_colons() {
        let msg = "Content-Type:application/json; charset=utf-8\r\n\r\n{}";
        let (headers, body) = split_msg(msg);
        assert_eq!(
            headers.get("Content-Type"),
            Some(&"application/json; charset=utf-8".to_string())
        );
        assert_eq!(body, "{}");
    }

    #[test]
    fn split_msg_empty_headers() {
        let (headers, body) = split_msg("\r\n\r\ntail");
        assert!(headers.is_empty());
        assert_eq!(body, "tail");
    }

    #[test]
    fn split_msg_duplicate_keys_last_wins() {
        let msg = "Path:a\r\nPath:b\r\n\r\n";
        let (headers, _) = split_msg(msg);
        assert_eq!(headers.get("Path"), Some(&"b".to_string()));
    }

    #[test]
    fn parse_word_boundaries_empty_metadata() {
        assert!(parse_word_boundaries(r#"{"Metadata":[]}"#).is_empty());
    }

    #[test]
    fn parse_word_boundaries_multiple_entries() {
        let body = r#"{"Metadata":[
            {"Type":"WordBoundary","Data":{"Offset":100,"Duration":50,"text":{"Text":"a"}}},
            {"Type":"WordBoundary","Data":{"Offset":200,"Duration":50,"text":{"Text":"b"}}},
            {"Type":"SentenceBoundary","Data":{"Offset":0,"Duration":0,"text":{"Text":"skip"}}}
        ]}"#;
        let events = parse_word_boundaries(body);
        assert_eq!(events.len(), 2);
    }
}
