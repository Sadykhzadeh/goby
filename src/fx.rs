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

/// RBJ biquad, transposed direct form II.
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

pub enum Shape {
    LowPass,
    HighPass,
    BandPass,
}

impl Biquad {
    pub fn new(shape: Shape, sample_rate: f32, freq: f32, q: f32) -> Self {
        let w = TAU * (freq / sample_rate).min(0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match shape {
            Shape::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            Shape::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
            Shape::BandPass => (alpha, 0.0, -alpha),
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Power-of-two delay line with fractional (linear) reads.
pub struct Delay {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
}

impl Delay {
    pub fn new(max_samples: usize) -> Self {
        let len = (max_samples + 2).next_power_of_two();
        Self { buf: vec![0.0; len], mask: len - 1, write: 0 }
    }

    #[inline]
    pub fn push(&mut self, x: f32) {
        self.write = (self.write + 1) & self.mask;
        self.buf[self.write] = x;
    }

    /// The sample `delay` samples ago (0 = the one just pushed).
    #[inline]
    pub fn read(&self, delay: f32) -> f32 {
        let d = delay.clamp(0.0, self.mask as f32 - 1.0);
        let i = d as usize;
        let t = d - i as f32;
        let a = self.buf[(self.write.wrapping_sub(i)) & self.mask];
        let b = self.buf[(self.write.wrapping_sub(i + 1)) & self.mask];
        a + (b - a) * t
    }

    pub fn clear(&mut self) {
        self.buf.fill(0.0);
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

/// A unit sine of `hz` at sample `i`, for tests.
#[cfg(test)]
pub fn sine(hz: f32, i: usize, sample_rate: f32) -> f32 {
    (i as f32 * hz * TAU / sample_rate).sin()
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

    #[test]
    fn delay_reads_back_what_it_was_given() {
        let mut d = Delay::new(100);
        for i in 0..50 {
            d.push(i as f32);
        }
        assert_eq!(d.read(0.0), 49.0);
        assert_eq!(d.read(10.0), 39.0);
        assert!((d.read(10.5) - 38.5).abs() < 1e-6);
    }

    #[test]
    fn filters_pass_and_cut_where_expected() {
        let level = |shape: fn() -> Biquad, hz: f32| {
            let mut f = shape();
            peak((0..SR as usize / 2).map(|i| f.process(sine(hz, i, SR))).skip(SR as usize / 10))
        };
        let lp = || Biquad::new(Shape::LowPass, SR, 1000.0, 0.707);
        let hp = || Biquad::new(Shape::HighPass, SR, 1000.0, 0.707);
        assert!(level(lp, 100.0) > 0.9 && level(lp, 8000.0) < 0.05);
        assert!(level(hp, 8000.0) > 0.9 && level(hp, 100.0) < 0.05);
    }
}
