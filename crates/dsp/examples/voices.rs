//! Times every voice on 60 s of audio: `cargo run --release -p goby-dsp --example voices`.
//! Prints the median of five runs per voice as nanoseconds per sample and as a real-time
//! factor (seconds of audio per second of compute), at the workspace's release settings.

use std::time::Instant;

use goby_dsp::dsp::Voice;
use goby_dsp::fx::Noise;

#[path = "../tests/support/voices.rs"]
mod voices;

const SR: u32 = 48_000;
const BLOCK: usize = 480;
const SECONDS: usize = 60;

fn input() -> Vec<f32> {
    let mut noise = Noise::default();
    (0..SECONDS * SR as usize)
        .map(|i| {
            let t = i as f32 / SR as f32;
            let burst = if (t % 4.0) < 0.5 { 0.5 * noise.next() } else { 0.0 };
            let phase = std::f32::consts::TAU * (100.0 * t + 975.0 * (t % 2.0) * (t % 2.0));
            burst + 0.4 * phase.sin()
        })
        .collect()
}

fn main() {
    let x = input();
    let mut out = x.clone();
    for (name, recipe, _) in voices::CASES {
        let mut runs: Vec<f64> = (0..5)
            .map(|_| {
                out.copy_from_slice(&x);
                let start = Instant::now();
                let mut voice = Voice::new(SR);
                voice.set(recipe, 0.0, 1.0);
                for block in out.chunks_mut(BLOCK) {
                    voice.process(block);
                }
                let elapsed = start.elapsed().as_secs_f64();
                std::hint::black_box(&out);
                elapsed
            })
            .collect();
        runs.sort_by(f64::total_cmp);
        let secs = runs[2];
        let ns = secs * 1e9 / x.len() as f64;
        println!("{name:<16} {ns:8.2} ns/sample  {:9.0}x real time", SECONDS as f64 / secs);
    }
}
