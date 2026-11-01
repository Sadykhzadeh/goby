//! Small effect building blocks the voices are made of. All of them allocate once in
//! `new` and then run per sample in the audio callback without locks or allocation.

use std::f32::consts::TAU;

/// Sine LFO.
#[derive(Clone, Copy, Default)]
pub struct Lfo {
    phase: f32,
}

impl Lfo {
    #[inline]
    pub fn next(&mut self, hz: f32, sample_rate: f32) -> f32 {
        self.phase = (self.phase + hz / sample_rate).fract();
        (self.phase * TAU).sin()
    }
}

/// xorshift32 white noise in -1..1; deterministic, cheap, good enough for audio.
#[derive(Clone, Copy)]
pub struct Noise(u32);

impl Default for Noise {
    fn default() -> Self {
        Noise(0x9e37_79b9)
    }
}

impl Noise {
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Slowly wandering random value in -1..1 (smoothed noise), for "drunk" and "mutation" wobble.
#[derive(Clone, Copy, Default)]
pub struct Wander {
    noise: Noise,
    value: f32,
    target: f32,
    countdown: u32,
}

impl Wander {
    #[inline]
    pub fn next(&mut self, hz: f32, sample_rate: f32) -> f32 {
        if self.countdown == 0 {
            self.target = self.noise.next();
            self.countdown = (sample_rate / hz.max(0.01)) as u32;
        }
        self.countdown -= 1;
        self.value += (self.target - self.value) * (hz * 2.0 / sample_rate).min(1.0);
        self.value
    }
}

/// Soft saturation; `drive` 1 is gentle, 10 is fuzz.
#[inline]
pub fn saturate(x: f32, drive: f32) -> f32 {
    (x * drive).tanh() / drive.tanh().max(1e-3)
}

/// Converts semitones (plus cents) to a playback ratio.
#[inline]
pub fn ratio(semitones: f32) -> f32 {
    2f32.powf(semitones / 12.0)
}

#[inline]
pub fn db(gain_db: f32) -> f32 {
    10f32.powf(gain_db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn peak(x: impl Iterator<Item = f32>) -> f32 {
        x.fold(0.0, |m, s| m.max(s.abs()))
    }

    #[test]
    fn lfo_and_noise_stay_in_range() {
        let mut l = Lfo::default();
        let mut n = Noise::default();
        let mut w = Wander::default();
        for _ in 0..100_000 {
            assert!(l.next(5.0, SR).abs() <= 1.0);
            assert!(n.next().abs() <= 1.0);
            assert!(w.next(3.0, SR).abs() <= 1.0);
        }
    }
}
