//! Inputs no microphone sends on purpose, for every voice: silence, a full-scale square,
//! one impulse, a non-finite sample from the driver, and the other sample rates the devices
//! hand out. The sweeps and noise of `voices_golden` are not repeated here.

use goby_dsp::dsp::{Recipe, Voice};
use goby_dsp::fx::Noise;
use goby_dsp::mic::Mic;

#[path = "support/voices.rs"]
mod voices;
use voices::CASES;

const SR: u32 = 48_000;

fn run(recipe: &Recipe, rate: u32, input: &[f32], block: usize) -> Vec<f32> {
    let mut voice = Voice::new(rate);
    voice.set(recipe, 0.0, 1.0);
    let mut x = input.to_vec();
    for part in x.chunks_mut(block) {
        voice.process(part);
    }
    x
}

/// A seeded noise burst from 0.5 s to 1 s over a sine sweep from 100 to 4000 Hz, at `rate`.
fn voiced(rate: u32, seconds: f32) -> Vec<f32> {
    let mut noise = Noise::default();
    (0..(seconds * rate as f32) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let burst = if (0.5..1.0).contains(&t) { 0.5 * noise.next() } else { 0.0 };
            let phase = std::f32::consts::TAU * (100.0 * t + 975.0 * t * t);
            burst + 0.4 * phase.sin()
        })
        .collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

#[test]
fn silence_gives_exact_silence_for_every_voice() {
    let silence = vec![0.0f32; SR as usize];
    for (name, recipe, _) in CASES {
        let out = run(recipe, SR, &silence, 480);
        assert_eq!(peak(&out), 0.0, "{name} makes sound out of silence");
    }
}

/// "Off" passes the microphone through bit for bit, so only the voices that process the
/// sound are held to full scale when the input is over it. The first 10 ms are left out:
/// a voice fades in from the dry passthrough, which is not limited, and that fade is part
/// of the voice's own start-up.
#[test]
fn a_full_scale_square_and_an_overdriven_one_stay_within_full_scale() {
    for amplitude in [1.0f32, 4.0] {
        let square: Vec<f32> =
            (0..SR as usize).map(|i| if (i / 240) % 2 == 0 { amplitude } else { -amplitude }).collect();
        for (name, recipe, _) in CASES {
            let out = run(recipe, SR, &square, 480);
            assert!(out.iter().all(|s| s.is_finite()), "{name} at {amplitude}x is not finite");
            if *recipe != Recipe::DRY {
                let settled = peak(&out[SR as usize / 100..]);
                assert!(settled <= 1.0, "{name} at {amplitude}x leaves full scale: {settled}");
            }
        }
    }
}

#[test]
fn a_single_impulse_dies_away_for_every_voice() {
    let mut impulse = vec![0.0f32; SR as usize * 4];
    impulse[0] = 1.0;
    for (name, recipe, _) in CASES {
        let out = run(recipe, SR, &impulse, 480);
        assert!(out.iter().all(|s| s.is_finite()), "{name} is not finite");
        let tail = peak(&out[SR as usize * 3..]);
        assert!(tail < 1e-4, "{name} still rings at {tail} three seconds after the impulse");
    }
}

/// The microphone path replaces a non-finite sample with silence before any voice sees it.
#[test]
fn a_nan_or_infinity_from_the_driver_never_reaches_the_output() {
    let mut input = voiced(SR, 1.0);
    (input[1000], input[2000]) = (f32::NAN, f32::INFINITY);
    for (name, recipe, _) in CASES {
        let (mut mic, mut voice) = (Mic::new(SR), Voice::new(SR));
        voice.set(recipe, 0.0, 1.0);
        let mut x = input.clone();
        for part in x.chunks_mut(480) {
            mic.process(part, 0.0, Some(-40.0));
            voice.process(part);
        }
        assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "{name}");
    }
}

#[test]
fn every_voice_is_finite_and_bounded_at_the_device_rates() {
    for rate in [44_100u32, 48_000, 96_000] {
        let input = voiced(rate, 1.0);
        for (name, recipe, _) in CASES {
            let out = run(recipe, rate, &input, 480);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "{name} at {rate} Hz: {}", peak(&out));
        }
    }
}

/// Off has no voice of its own, so a volume above unity is the only thing that can take it past
/// full scale. Its output is eased above 0.99 at every volume, as the voices' tanh eases theirs:
/// at unity only the peaks above that knee move, and by less than 0.005.
#[test]
fn off_at_a_boost_stays_within_full_scale_and_is_near_exact_at_unity() {
    let mut heard = voiced(SR, 1.0);
    Mic::new(SR).process(&mut heard, 24.0, None);
    assert!(peak(&heard) <= 1.0 && peak(&heard) > 0.9, "the microphone stage: {}", peak(&heard));

    let mut unity = Voice::new(SR);
    unity.set(&Recipe::DRY, 0.0, 1.0);
    let mut exact = heard.clone();
    unity.process(&mut exact);
    let moved = exact.iter().zip(&heard).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(moved < 0.005, "at unity the passthrough moves a sample by {moved}");

    let mut boost = Voice::new(SR);
    boost.set(&Recipe::DRY, 0.0, 2.0);
    let mut out = heard.clone();
    boost.process(&mut out);
    assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "a boosted Off reaches {}", peak(&out));
    assert!(peak(&out) > 0.95, "the boost is not heard: {}", peak(&out));
}
