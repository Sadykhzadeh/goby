//! Goby on the website: the app's own voices and its window, built for wasm32 from the
//! app's source files unchanged. One module serves both: the page runs the window
//! (`window.rs`, a thin shim for docs/window.js), and an AudioWorklet runs the voices
//! (the plain exports below, for docs/worklet.js).
//!
//! The site's deploy builds it, and the page, from the commit it publishes. The same, to
//! preview the site locally, from this folder:
//!
//!   cargo build --release --locked --target wasm32-unknown-unknown --lib
//!   cp target/wasm32-unknown-unknown/release/goby_dsp_wasm.wasm ../../docs/goby.wasm
//!   cargo run --release --locked --bin site
//!
//! No wasm-bindgen: the JavaScript calls plain exports and supplies plain imports.
//! One instance per thread, so the state lives in statics; calls come one at a time.
#![allow(dead_code)]

#[cfg(test)]
// The app's voice tests name the crate goby_dsp; this makes that name refer to this crate.
extern crate self as goby_dsp;

#[path = "../../../crates/dsp/src/dsp.rs"]
mod dsp;
#[path = "../../../crates/dsp/src/formant.rs"]
mod formant;
#[path = "../../../crates/dsp/src/fx.rs"]
mod fx;
#[path = "../../../crates/dsp/src/mic.rs"]
mod mic;
#[path = "../../../crates/dsp/src/ring.rs"]
mod ring;
#[path = "../../../crates/dsp/src/stream.rs"]
mod stream;
#[path = "../../../src/voices.rs"]
mod voices;

#[path = "../../../src/demo.rs"]
mod demo;
#[path = "../../../src/draw.rs"]
mod draw;
#[path = "../../../src/host.rs"]
mod host;
#[path = "../../../src/lang.rs"]
mod lang;
#[path = "../../../src/settings.rs"]
mod settings;
#[path = "../../../src/theme.rs"]
mod theme;
#[path = "../../../src/ui/mod.rs"]
mod ui;
mod window;

use dsp::Voice;
use mic::Mic;
use stream::Blend;

/// Largest block `process` takes; an AudioWorklet render quantum is 128 frames.
const MAX_BLOCK: usize = 1024;

struct Engine {
    voice: Voice,
    mic: Mic,
    blend: Blend,
    block: [f32; MAX_BLOCK],
    dry: [f32; MAX_BLOCK],
    chosen: usize,
    tone: f32,
    volume: f32,
    enabled: bool,
    gain_db: f32,
    gate_db: Option<f32>,
    mix: f32,
}

static mut ENGINE: Option<Engine> = None;

#[allow(static_mut_refs)]
fn engine() -> &'static mut Engine {
    unsafe { ENGINE.as_mut().expect("init first") }
}

/// Sets up the engine for the context's sample rate and returns the address of the
/// block that `process` reads and writes.
#[unsafe(no_mangle)]
pub extern "C" fn init(sample_rate: u32) -> *mut f32 {
    let e = Engine {
        voice: Voice::new(sample_rate),
        mic: Mic::new(sample_rate),
        blend: Blend::new(sample_rate),
        block: [0.0; MAX_BLOCK],
        dry: [0.0; MAX_BLOCK],
        chosen: 0,
        tone: 0.0,
        volume: 1.0,
        enabled: true,
        gain_db: 0.0,
        gate_db: None,
        mix: 0.0,
    };
    unsafe { ENGINE = Some(e) };
    engine().block.as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn voice_count() -> usize {
    voices::ALL.len()
}

/// The voice by its place in the catalog, the pitch in semitones and the output volume.
#[unsafe(no_mangle)]
pub extern "C" fn set_voice(index: usize, tone: f32, volume: f32) {
    let e = engine();
    e.chosen = index.min(voices::ALL.len() - 1);
    e.tone = tone;
    e.volume = volume;
}

/// The microphone side: make-up gain, the gate threshold (NaN leaves the gate open),
/// the share of the original voice mixed back in, and whether the effect is on. A gain or
/// mix that is not finite counts as 0 dB or 0: one NaN would stay in the level and the
/// blend for good, and every later block would come out silent.
#[unsafe(no_mangle)]
pub extern "C" fn set_mic(gain_db: f32, gate_db: f32, mix: f32, enabled: bool) {
    let e = engine();
    e.gain_db = if gain_db.is_finite() { gain_db } else { 0.0 };
    e.gate_db = Some(gate_db).filter(|g| !g.is_nan());
    e.mix = if mix.is_finite() { mix } else { 0.0 };
    e.enabled = enabled;
}

/// Runs the first `n` samples of the block in place, the same way the app's audio
/// thread does: gate, voice, then the original mixed back in.
#[unsafe(no_mangle)]
pub extern "C" fn process(n: usize) {
    let e = engine();
    let n = n.min(MAX_BLOCK);
    let def = &voices::ALL[if e.enabled { e.chosen } else { 0 }];
    let tone = if e.enabled { e.tone } else { 0.0 };
    let mix = if e.enabled { e.mix } else { 0.0 };
    e.voice.set(&def.recipe(), tone, e.volume);
    let chunk = &mut e.block[..n];
    e.mic.process(chunk, e.gain_db, e.gate_db);
    e.dry[..n].copy_from_slice(chunk);
    e.voice.process(chunk);
    e.blend.process(chunk, &e.dry[..n], mix);
}

/// How far the voice runs behind the microphone right now, in samples.
#[unsafe(no_mangle)]
pub extern "C" fn delay() -> f32 {
    engine().voice.delay()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loudest sample after a few blocks of a steady 0.5 input.
    fn peak() -> f32 {
        let e = engine();
        let mut peak = 0.0f32;
        for _ in 0..8 {
            e.block[..256].fill(0.5);
            process(256);
            peak = e.block[..256].iter().fold(peak, |p, s| p.max(s.abs()));
        }
        peak
    }

    #[test]
    fn a_non_finite_gain_or_mix_does_not_silence_the_output_for_good() {
        init(48_000);
        set_mic(f32::NAN, f32::NAN, f32::NAN, true);
        assert!(peak().is_finite());
        set_mic(0.0, f32::NAN, 0.0, true);
        assert!(peak() > 0.1, "a NaN gain kept the output silent after a valid gain was set");
        set_mic(f32::INFINITY, f32::NAN, f32::INFINITY, true);
        set_mic(0.0, f32::NAN, 0.0, true);
        assert!(peak() > 0.1, "an infinite gain kept the output silent after a valid gain was set");
    }
}
