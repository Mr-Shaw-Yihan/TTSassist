// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! The `Engine` trait - the swappable TTS-backend contract.
//!
//! `EdgeTts` is the reference implementation.  Consumers that want to mock
//! the engine in tests or swap in a different provider implement this trait.

use crate::error::TtsError;
use crate::event::TtsEvent;
use std::future::Future;

/// A text-to-speech backend.
///
/// Implementations must be `Send + Sync` so the engine can live behind an
/// `Arc` on a worker thread.  The returned future must be `Send` so it can
/// cross thread boundaries when driven via control channels.
///
/// # Arguments
///
/// * `text`  - one utterance (callers chunk longer text; the Edge endpoint
///   caps a single request near ~4 KB).
/// * `voice` - a full voice short-name, e.g. `"en-US-EmmaMultilingualNeural"`.
/// * `rate`  - an SSML prosody rate string: `"+0%"`, `"+25%"`, `"-10%"`.
/// * `lang`  - BCP-47 language tag for the `xml:lang` attribute, e.g. `"en-US"`.
pub trait Engine: Send + Sync {
    fn synthesize(
        &self,
        text: &str,
        voice: &str,
        rate: &str,
        lang: &str,
    ) -> impl Future<Output = Result<Vec<TtsEvent>, TtsError>> + Send;
}
