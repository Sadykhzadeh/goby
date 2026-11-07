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

#[path = "../../../src/dsp.rs"]
mod dsp;
#[path = "../../../src/fx.rs"]
mod fx;
#[path = "../../../src/mic.rs"]
mod mic;
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

/// Largest block `process` takes; an AudioWorklet render quantum is 128 frames.
const MAX_BLOCK: usize = 1024;

struct Engine {
    voice: Voice,
    mic: Mic,
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
/// the share of the original voice mixed back in, and whether the effect is on.
#[unsafe(no_mangle)]
pub extern "C" fn set_mic(gain_db: f32, gate_db: f32, mix: f32, enabled: bool) {
    let e = engine();
    e.gain_db = gain_db;
    e.gate_db = Some(gate_db).filter(|g| !g.is_nan());
    e.mix = mix;
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
    e.voice.set(&def.recipe, tone, e.volume);
    let chunk = &mut e.block[..n];
    e.mic.process(chunk, e.gain_db, e.gate_db);
    if mix > 0.0 {
        e.dry[..n].copy_from_slice(chunk);
    }
    e.voice.process(chunk);
    if mix > 0.0 {
        for (s, d) in chunk.iter_mut().zip(&e.dry[..n]) {
            *s = *s * (1.0 - mix) + d * mix;
        }
    }
}

/// How far the voice runs behind the microphone right now, in samples.
#[unsafe(no_mangle)]
pub extern "C" fn delay() -> f32 {
    engine().voice.delay()
}
