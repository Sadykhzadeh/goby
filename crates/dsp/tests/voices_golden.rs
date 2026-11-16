//! Checks on every voice, rendered from one fixed input.
//!
//! The golden table holds the exact bits each voice produced when the table was made. The
//! voices call libm (`sin`, `tanh`, `powf`), whose last bits differ between platforms and
//! libm versions, so the table is compared only on request (see the ignored test below).
//! The checks that run by default: every sample is finite and inside full scale, two runs
//! agree bit for bit, and the output does not depend on the block size.

use goby_dsp::dsp::{Recipe, Voice};
use goby_dsp::fx::Noise;

#[path = "support/voices.rs"]
mod voices;
use voices::CASES;

const SR: u32 = 48_000;
const BLOCK: usize = 480;

/// A seeded noise burst from 0.5 s to 1 s over a sine sweep from 100 to 4000 Hz.
fn input(seconds: usize) -> Vec<f32> {
    let mut noise = Noise::default();
    (0..seconds * SR as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let burst = if (0.5..1.0).contains(&t) { 0.5 * noise.next() } else { 0.0 };
            let phase = std::f32::consts::TAU * (100.0 * t + 975.0 * t * t);
            burst + 0.4 * phase.sin()
        })
        .collect()
}

fn fnv1a(samples: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn render(recipe: &Recipe, input: &[f32], block: usize) -> Vec<f32> {
    let mut voice = Voice::new(SR);
    voice.set(recipe, 0.0, 1.0);
    let mut x = input.to_vec();
    for part in x.chunks_mut(block) {
        voice.process(part);
    }
    x
}

/// Compares every voice with its golden hash. The hashes are tied to the libm they were made
/// with, so this is ignored by default. To check an optimisation, run it on the same machine
/// before and after the change:
/// `cargo test -p goby-dsp --test voices_golden -- --ignored`. To regenerate the table, run
/// `GOBY_GOLDEN_PRINT=1 cargo test -p goby-dsp --test voices_golden -- --ignored --nocapture`
/// and paste the printed hashes into `support/voices.rs`.
#[test]
#[ignore = "golden hashes depend on the libm they were made with; run with --ignored"]
fn every_voice_renders_the_golden_bits() {
    let x = input(2);
    if std::env::var_os("GOBY_GOLDEN_PRINT").is_some() {
        for (name, recipe, _) in CASES {
            println!("{name} {:016x}", fnv1a(&render(recipe, &x, BLOCK)));
        }
        return;
    }
    let mut changed = Vec::new();
    for (name, recipe, want) in CASES {
        let got = fnv1a(&render(recipe, &x, BLOCK));
        if got != *want {
            changed.push(format!("{name} {got:016x}"));
        }
    }
    assert!(changed.is_empty(), "voices whose output changed:\n{}", changed.join("\n"));
}

#[test]
fn every_voice_is_finite_within_full_scale_and_repeatable() {
    let x = input(1);
    for (name, recipe, _) in CASES {
        let first = render(recipe, &x, BLOCK);
        assert!(first.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "{name} leaves full scale");
        assert_eq!(fnv1a(&first), fnv1a(&render(recipe, &x, BLOCK)), "{name} differs between runs");
    }
}

/// Voices that snap their pitch to a scale take a new bend at the start of each block, so
/// their output depends on the block size. That is a known gap; they are left out here
/// until the bend is taken on a fixed sample grid.
#[test]
fn every_untuned_voice_is_independent_of_the_block_size() {
    let x = input(1);
    for (name, recipe, _) in CASES.iter().filter(|(_, r, _)| r.tune == 0) {
        let whole = fnv1a(&render(recipe, &x, BLOCK));
        for block in [64, 1024] {
            assert_eq!(fnv1a(&render(recipe, &x, block)), whole, "{name} changes with blocks of {block}");
        }
    }
}
