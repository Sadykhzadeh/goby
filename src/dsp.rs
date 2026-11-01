//! Real-time voice effects. Everything here runs inside the audio callback:
//! no allocation after construction, no locks, no syscalls.

use std::f32::consts::PI;

/// Delay-line pitch shifter with correlation-aligned jumps (WSOLA in a ring).
///
/// A read tap drifts through the delay line at `1 - ratio` samples per sample.
/// When it runs out of room it jumps by `lag` samples, and `lag` is picked so the
/// waveform at the new spot lines up with the old one; the short crossfade between
/// them then adds instead of cancelling. Formants move with pitch, which is exactly
/// the "chipmunk" colour the Minion and Helium voices want.
pub struct PitchShifter {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    ratio: f32,
    delay: f32,
    /// Delay of the incoming tap and progress of the crossfade towards it.
    fade: Option<(f32, usize)>,
    min_delay: f32,
    max_delay: f32,
    min_lag: usize,
    max_lag: usize,
    fade_len: usize,
    corr_len: usize,
}

impl PitchShifter {
    pub fn new(sample_rate: u32) -> Self {
        let ms = |m: f32| (sample_rate as f32 * m / 1000.0) as usize;
        // Lag range must hold a full period of a low voice (80 Hz = 12.5 ms).
        let (min_lag, max_lag, fade_len, corr_len) = (ms(6.0), ms(20.0), ms(5.0), ms(5.0));
        let min_delay = 4.0;
        // Room for the largest lag plus the drift during a fade at two octaves.
        let max_delay = min_delay + (max_lag + 4 * fade_len) as f32;
        let len = (max_delay as usize + corr_len + max_lag + 8).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
            write: 0,
            ratio: 1.0,
            delay: min_delay + max_lag as f32 / 2.0,
            fade: None,
            min_delay,
            max_delay,
            min_lag,
            max_lag,
            fade_len,
            corr_len,
        }
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(0.25, 4.0);
    }

    #[inline]
    fn at(&self, i: isize) -> f32 {
        // Negative indices wrap correctly because the length is a power of two.
        self.buf[i as usize & self.mask]
    }

    /// Lag that best lines up the audio behind `from` with the audio behind `from + sign*lag`.
    fn best_lag(&self, from: f32, sign: isize) -> usize {
        let base = self.write as isize - from as isize;
        let score = |lag: usize| {
            let other = base - sign * lag as isize;
            let (mut xy, mut yy) = (0.0f32, 1e-9f32);
            for j in 0..self.corr_len as isize {
                let (a, b) = (self.at(base - j), self.at(other - j));
                xy += a * b;
                yy += b * b;
            }
            xy / yy.sqrt()
        };
        // Coarse pass every 2 samples, then refine around the winner.
        let mut best = (self.min_lag, f32::MIN);
        for lag in (self.min_lag..=self.max_lag).step_by(2) {
            let s = score(lag);
            if s > best.1 {
                best = (lag, s);
            }
        }
        for lag in [best.0.saturating_sub(1), best.0 + 1] {
            if (self.min_lag..=self.max_lag).contains(&lag) {
                let s = score(lag);
                if s > best.1 {
                    best = (lag, s);
                }
            }
        }
        best.0
    }

    #[inline]
    fn tap(&self, delay: f32) -> f32 {
        // Cubic Hermite read `delay` samples behind the write head.
        let pos = self.write as f32 - delay;
        let i = pos.floor();
        let t = pos - i;
        let i = i as isize;
        let (y0, y1, y2, y3) = (self.at(i - 1), self.at(i), self.at(i + 1), self.at(i + 2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.buf[self.write] = x;
        let drift = 1.0 - self.ratio;
        let mut y = self.tap(self.delay);
        if let Some((next, t)) = self.fade {
            let g = 0.5 - 0.5 * (PI * t as f32 / self.fade_len as f32).cos();
            y += (self.tap(next) - y) * g;
            self.fade = if t + 1 >= self.fade_len {
                self.delay = next;
                None
            } else {
                Some((next + drift, t + 1))
            };
        }
        self.delay += drift;

        if self.fade.is_none() {
            let room = self.fade_len as f32 * drift.abs() + 1.0;
            if drift > 0.0 && self.delay > self.max_delay - room {
                let lag = self.best_lag(self.delay, -1);
                self.fade = Some((self.delay - lag as f32, 0));
            } else if drift < 0.0 && self.delay < self.min_delay + room {
                let lag = self.best_lag(self.delay, 1);
                self.fade = Some((self.delay + lag as f32, 0));
            }
        }
        // Kept masked: an ever-growing index would lose precision as f32 after a few minutes.
        self.write = (self.write + 1) & self.mask;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn sine(freq: f32, secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize).map(|i| (2.0 * PI * freq * i as f32 / SR as f32).sin() * 0.5).collect()
    }

    /// Dominant frequency via a coarse DFT scan; good to ~1 Hz, plenty for a ratio check.
    fn dominant_freq(x: &[f32]) -> f32 {
        let (mut best, mut best_f) = (0.0, 0.0);
        let mut f = 50.0;
        while f < 2000.0 {
            let w = 2.0 * PI * f / SR as f32;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, s) in x.iter().enumerate() {
                let (sn, cs) = (w * i as f32).sin_cos();
                re += s * cs;
                im += s * sn;
            }
            let m = re * re + im * im;
            if m > best {
                best = m;
                best_f = f;
            }
            f += 1.0;
        }
        best_f
    }

    fn shifted(freq: f32, semis: f32) -> f32 {
        let mut p = PitchShifter::new(SR);
        p.set_ratio(2f32.powf(semis / 12.0));
        let out: Vec<f32> = sine(freq, 1.0).into_iter().map(|s| p.process(s)).collect();
        dominant_freq(&out[SR as usize / 4..])
    }

    #[test]
    fn shifts_up_an_octave() {
        let f = shifted(220.0, 12.0);
        assert!((f - 440.0).abs() <= 4.0, "got {f}");
    }

    #[test]
    fn shifts_minion_interval() {
        let f = shifted(200.0, 8.0);
        assert!((f - 200.0 * 2f32.powf(8.0 / 12.0)).abs() <= 4.0, "got {f}");
    }

    #[test]
    fn shifts_down() {
        let f = shifted(300.0, -6.0);
        assert!((f - 300.0 * 2f32.powf(-0.5)).abs() <= 4.0, "got {f}");
    }

    #[test]
    fn unity_ratio_is_a_short_delay() {
        let mut p = PitchShifter::new(SR);
        let input = sine(440.0, 0.5);
        let out: Vec<f32> = input.iter().map(|s| p.process(*s)).collect();
        let peak = out[SR as usize / 10..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.02, "peak {peak}");
    }
}
