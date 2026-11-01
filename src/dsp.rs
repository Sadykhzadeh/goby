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

/// RBJ biquad, transposed direct form II.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new(sample_rate: u32, freq: f32, q: f32, highpass: bool) -> Self {
        let w = 2.0 * PI * freq / sample_rate as f32;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if highpass {
            ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0)
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0)
        };
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * cos / a0, a2: (1.0 - alpha) / a0, z1: 0.0, z2: 0.0 }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Off,
    Minion,
    Helium,
    Giant,
    Robot,
    Radio,
}

impl Preset {
    pub const ALL: [Preset; 6] =
        [Preset::Off, Preset::Minion, Preset::Helium, Preset::Giant, Preset::Robot, Preset::Radio];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|p| *p == self).unwrap()
    }

    pub fn from_index(i: usize) -> Preset {
        Self::ALL.get(i).copied().unwrap_or(Preset::Off)
    }

    /// Pitch the preset starts from, in semitones. The user can still move it.
    pub fn semitones(self) -> f32 {
        match self {
            Preset::Minion => 8.0,
            Preset::Helium => 12.0,
            Preset::Giant => -6.0,
            Preset::Off | Preset::Robot | Preset::Radio => 0.0,
        }
    }
}

/// The full chain: pitch → robot → radio → gain.
pub struct Voice {
    sample_rate: f32,
    pitch: PitchShifter,
    robot_phase: f32,
    comb: Vec<f32>,
    comb_pos: usize,
    hp: Biquad,
    lp: Biquad,
    lp2: Biquad,
    preset: Preset,
    semitones: f32,
    gain: f32,
}

impl Voice {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate as f32,
            pitch: PitchShifter::new(sample_rate),
            robot_phase: 0.0,
            // 6 ms metallic comb for the robot.
            comb: vec![0.0; (sample_rate as f32 * 0.006) as usize],
            comb_pos: 0,
            hp: Biquad::new(sample_rate, 450.0, 0.707, true),
            lp: Biquad::new(sample_rate, 2800.0, 0.707, false),
            lp2: Biquad::new(sample_rate, 2800.0, 0.707, false),
            preset: Preset::Off,
            semitones: 0.0,
            gain: 1.0,
        }
    }

    pub fn set(&mut self, preset: Preset, semitones: f32, gain: f32) {
        self.preset = preset;
        self.gain = gain;
        if semitones != self.semitones {
            self.semitones = semitones;
            self.pitch.set_ratio(2f32.powf(semitones / 12.0));
        }
    }

    pub fn process(&mut self, block: &mut [f32]) {
        let off = self.preset == Preset::Off;
        let shift = !off && self.semitones.abs() > 0.01;
        for s in block.iter_mut() {
            let mut x = *s;
            if shift {
                x = self.pitch.process(x);
            }
            match self.preset {
                Preset::Robot => {
                    self.robot_phase = (self.robot_phase + 55.0 / self.sample_rate) % 1.0;
                    x *= (2.0 * PI * self.robot_phase).sin() * 1.6;
                    let delayed = self.comb[self.comb_pos];
                    x += delayed * 0.55;
                    self.comb[self.comb_pos] = x;
                    self.comb_pos = (self.comb_pos + 1) % self.comb.len();
                }
                Preset::Radio => {
                    x = self.lp2.process(self.lp.process(self.hp.process(x)));
                    x = (x * 4.0).tanh() * 0.6;
                }
                _ => {}
            }
            // Soft limit so a loud scream never clips hard in voice chat.
            *s = if off { x * self.gain } else { (x * self.gain).tanh() };
        }
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

    #[test]
    fn every_preset_stays_finite_and_bounded() {
        for preset in Preset::ALL {
            let mut v = Voice::new(SR);
            v.set(preset, preset.semitones(), 1.0);
            let mut block = sine(180.0, 0.5);
            block.iter_mut().for_each(|s| *s *= 2.0);
            v.process(&mut block);
            assert!(block.iter().all(|s| s.is_finite() && s.abs() <= 1.0 + f32::EPSILON), "{preset:?}");
        }
    }

    #[test]
    fn radio_cuts_lows() {
        let mut v = Voice::new(SR);
        v.set(Preset::Radio, 0.0, 1.0);
        let mut low = sine(80.0, 0.5);
        v.process(&mut low);
        let peak = low[SR as usize / 10..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.15, "80 Hz leaked at {peak}");
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// `cargo test --release bench -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark"]
    fn realtime_factor() {
        let sr = 48_000;
        let input: Vec<f32> =
            (0..sr * 10).map(|i| (i as f32 * 0.0291).sin() * 0.4 + (i as f32 * 0.071).sin() * 0.2).collect();
        for preset in Preset::ALL {
            let mut v = Voice::new(sr as u32);
            v.set(preset, preset.semitones(), 1.0);
            let mut buf = input.clone();
            let t = std::time::Instant::now();
            for block in buf.chunks_mut(480) {
                v.process(block);
            }
            let secs = t.elapsed().as_secs_f64();
            println!("{preset:?}: 10 s of audio in {:.2} ms → {:.3} % of one core", secs * 1000.0, secs / 10.0 * 100.0);
        }
    }
}
