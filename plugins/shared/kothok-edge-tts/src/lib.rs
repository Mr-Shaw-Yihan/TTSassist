// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
#![doc = include_str!("../README.md")]

mod auth;
mod connection;
mod edge_tts;
mod error;
mod event;
mod protocol;
mod rate;
mod ssml;
mod token_v2;
mod tts;
mod voice_cache;
mod voice_fetch;
mod voice_table;
mod voice_table_data;
mod voice_types;

pub use edge_tts::EdgeTts;
pub use error::TtsError;
pub use event::TtsEvent;
pub use tts::Engine;
pub use voice_cache::{load_voice_cache, save_voice_cache};
pub use voice_fetch::{list_voices, spawn_voice_fetch};
pub use voice_table::{
    normalize_lang, voice_label, voices_for_lang, DEFAULT_VOICE_BN, DEFAULT_VOICE_EN,
};
pub use voice_types::{set_dynamic_voices, VoiceEntry, VoiceInfo};

pub use rate::{rate_percent, rate_string, RATE_BASELINE_OFFSET};

#[doc(hidden)]
pub use auth::{civil_utc, date_string, random_hex, sec_ms_gec};

/// Install rustls's `ring` crypto provider.
///
/// rustls 0.23 requires an explicit crypto provider before any TLS handshake.
/// Call this **once** at startup, before the first [`EdgeTts`] connect.
/// Idempotent - safe to call multiple times.
pub fn init_tls() {
    // best-effort: idempotent - the provider may already be installed by the host
    let _ = rustls::crypto::ring::default_provider().install_default();
}
