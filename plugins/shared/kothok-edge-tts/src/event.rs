// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Nayeem Bin Ahsan
//! Streaming events emitted during a single synthesis turn.

/// One item in the event stream produced by [`crate::Engine::synthesize`].
///
/// Events arrive in order. A well-formed turn starts with zero or more
/// [`Audio`](TtsEvent::Audio) / [`WordBoundary`](TtsEvent::WordBoundary)
/// events and terminates with exactly one [`TurnEnd`](TtsEvent::TurnEnd).
#[derive(Debug, Clone)]
pub enum TtsEvent {
    /// A chunk of raw MP3 audio bytes (`audio-24khz-48kbitrate-mono-mp3`).
    /// Concatenate all `Audio` events in order to reconstruct the full
    /// utterance.
    Audio(Vec<u8>),

    /// Word-boundary timing metadata for highlight / karaoke effects.
    /// `offset` and `duration` are in **100-nanosecond ticks** relative to
    /// the start of the audio stream.
    WordBoundary {
        offset: u64,
        duration: u64,
        text: String,
    },

    /// The server signalled `turn.end` - no more audio or metadata will
    /// arrive for this request.
    TurnEnd,
}
