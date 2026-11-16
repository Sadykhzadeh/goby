//! Golden-output net for the voices. Each voice renders a fixed input through the
//! same blocks and must produce the same bits as when its hash in `support/voices.rs`
//! was made, so any change that moves a single sample of any voice fails here. The hash
//! is FNV-1a over the samples' bit patterns. The sine uses the platform's libm, so the
//! table is for the Linux build that CI runs.

use goby_dsp::dsp::{Recipe, Voice};
use goby_dsp::fx::Noise;

#[path = "support/voices.rs"]
mod voices;
use voices::CASES;

const SR: u32 = 48_000;
const BLOCK: usize = 480;

/// Two seconds: a seeded noise burst from 0.5 s to 1 s over a sine sweep from 100 to 4000 Hz.
fn input() -> Vec<f32> {
    let mut noise = Noise::default();
    (0..2 * SR as usize)
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

fn render(recipe: &Recipe) -> u64 {
    let mut voice = Voice::new(SR);
    voice.set(recipe, 0.0, 1.0);
    let mut x = input();
    for block in x.chunks_mut(BLOCK) {
        voice.process(block);
    }
    fnv1a(&x)
}

#[test]
fn every_voice_renders_the_golden_bits() {
    let mut changed = Vec::new();
    for (name, recipe, want) in CASES {
        let got = render(recipe);
        if got != *want {
            changed.push(format!("{name} {got:016x}"));
        }
    }
    assert!(changed.is_empty(), "voices whose output changed:\n{}", changed.join("\n"));
}
