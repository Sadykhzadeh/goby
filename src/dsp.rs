//! Real-time voice effects. Everything here runs inside the audio callback:
//! no allocation after construction, no locks, no syscalls.

use std::f32::consts::PI;

use crate::fx::{
    Biquad, Crusher, Delay, Echo, Envelope, Lfo, Modulated, Noise, Reverb, Shape, Vocoder, Wander, ratio, saturate,
};

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
    min_lag: usize,
    max_lag: usize,
    fade_len: usize,
    corr_len: usize,
    /// Lag of the previous jump: a good guess at the current pitch period.
    last_lag: usize,
    /// Samples spent waiting for a good enough match before a downward jump.
    wait: u32,
}

impl PitchShifter {
    pub fn new(sample_rate: u32) -> Self {
        let ms = |m: f32| (sample_rate as f32 * m / 1000.0) as usize;
        // The lag range only has to hold one full period of a low voice (80 Hz = 12.5 ms);
        // every extra millisecond here is a millisecond of latency.
        let (min_lag, max_lag, fade_len, corr_len) = (ms(6.0), ms(13.5), ms(5.0), ms(5.0));
        let min_delay = 4.0;
        // Deepest the tap can go: largest lag plus drift during a fade, twice, at two octaves.
        let deepest = min_delay as usize + max_lag + 6 * fade_len;
        let len = (deepest + corr_len + max_lag + 8).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
            write: 0,
            ratio: 1.0,
            delay: min_delay + max_lag as f32 / 2.0,
            fade: None,
            min_delay,
            min_lag,
            max_lag,
            fade_len,
            corr_len,
            last_lag: max_lag,
            wait: 0,
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

    /// Lag (at most `longest`) that best lines up the audio behind `from` with the audio
    /// behind `from + sign*lag`, and its normalised correlation in -1..=1.
    fn best_lag(&self, from: f32, sign: isize, longest: usize) -> (usize, f32) {
        let longest = longest.clamp(self.min_lag, self.max_lag);
        let base = self.write as isize - from as isize;
        let score = |lag: usize| {
            let other = base - sign * lag as isize;
            let (mut xy, mut xx, mut yy) = (0.0f32, 1e-9f32, 1e-9f32);
            for j in 0..self.corr_len as isize {
                let (a, b) = (self.at(base - j), self.at(other - j));
                xy += a * b;
                xx += a * a;
                yy += b * b;
            }
            xy / (xx * yy).sqrt()
        };
        // Coarse pass every 2 samples, then refine around the winner.
        let mut best = (self.min_lag, f32::MIN);
        for lag in (self.min_lag..=longest).step_by(2) {
            let s = score(lag);
            if s > best.1 {
                best = (lag, s);
            }
        }
        for lag in [best.0.saturating_sub(1), best.0 + 1] {
            if (self.min_lag..=longest).contains(&lag) {
                let s = score(lag);
                if s > best.1 {
                    best = (lag, s);
                }
            }
        }
        best
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
            if drift > 0.0 {
                // Shifting down the tap falls behind. Jump forward as soon as one period
                // (the last lag) fits, if the waveform lines up well; only when the full lag
                // range fits is the best match taken regardless. This keeps shifting down
                // about as cheap in latency as shifting up.
                let reach = self.delay - self.min_delay - room;
                let forced = reach >= self.max_lag as f32;
                if reach >= self.last_lag as f32 {
                    self.wait += 1;
                    if forced || self.wait % 32 == 1 {
                        let (lag, score) = self.best_lag(self.delay, -1, reach as usize);
                        if forced || score > 0.6 {
                            self.last_lag = lag;
                            self.wait = 0;
                            self.fade = Some((self.delay - lag as f32, 0));
                        }
                    }
                }
            } else if drift < 0.0 && self.delay < self.min_delay + room {
                let (lag, _) = self.best_lag(self.delay, 1, self.max_lag);
                self.last_lag = lag;
                self.fade = Some((self.delay + lag as f32, 0));
            }
        }
        // Kept masked: an ever-growing index would lose precision as f32 after a few minutes.
        self.write = (self.write + 1) & self.mask;
        y
    }
}

/// Everything a voice is made of. Zero switches a stage off, so a recipe only names what it uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    /// Pitch shift in semitones; formants move with it (the chipmunk / giant colour).
    pub pitch: f32,
    /// Up to two extra pitched copies of the voice, in semitones (0 = unused).
    pub harmony: [f32; 2],
    pub harmony_mix: f32,
    pub vibrato_hz: f32,
    pub vibrato_cents: f32,
    /// Slow random pitch drift: drunk, nervous, mutating.
    pub wobble_hz: f32,
    pub wobble_cents: f32,
    pub ring_hz: f32,
    pub ring_mix: f32,
    /// Replaces the voiced buzz with noise while keeping the vowel shapes.
    pub whisper: f32,
    /// Carrier root of the vocoder in Hz (0 = no vocoder).
    pub vocoder_hz: f32,
    pub vocoder_chord: bool,
    /// Short feedback comb: metallic, tin-can, robotic.
    pub comb_ms: f32,
    pub comb_feedback: f32,
    pub chorus: f32,
    pub flanger_hz: f32,
    pub flanger_mix: f32,
    pub tremolo_hz: f32,
    pub tremolo_depth: f32,
    pub highpass_hz: f32,
    pub lowpass_hz: f32,
    /// Saturation drive (0 = clean); `hard_clip` squares it off instead.
    pub drive: f32,
    pub hard_clip: bool,
    /// Sample-and-hold rate in Hz and bit depth (0 = clean).
    pub crush_hz: f32,
    pub crush_bits: u32,
    /// Radio static that only plays while you talk.
    pub hiss: f32,
    pub echo_ms: f32,
    pub echo_feedback: f32,
    pub echo_mix: f32,
    pub reverb_size: f32,
    pub reverb_damp: f32,
    pub reverb_mix: f32,
    /// Make-up gain for recipes that lose or add a lot of level.
    pub gain: f32,
}

impl Recipe {
    pub const DRY: Recipe = Recipe {
        pitch: 0.0,
        harmony: [0.0; 2],
        harmony_mix: 0.0,
        vibrato_hz: 0.0,
        vibrato_cents: 0.0,
        wobble_hz: 0.0,
        wobble_cents: 0.0,
        ring_hz: 0.0,
        ring_mix: 0.0,
        whisper: 0.0,
        vocoder_hz: 0.0,
        vocoder_chord: false,
        comb_ms: 0.0,
        comb_feedback: 0.0,
        chorus: 0.0,
        flanger_hz: 0.0,
        flanger_mix: 0.0,
        tremolo_hz: 0.0,
        tremolo_depth: 0.0,
        highpass_hz: 0.0,
        lowpass_hz: 0.0,
        drive: 0.0,
        hard_clip: false,
        crush_hz: 0.0,
        crush_bits: 0,
        hiss: 0.0,
        echo_ms: 0.0,
        echo_feedback: 0.0,
        echo_mix: 0.0,
        reverb_size: 0.0,
        reverb_damp: 0.0,
        reverb_mix: 0.0,
        gain: 1.0,
    };
}

/// Runs one recipe. Every stage is allocated up front, so switching voices in the audio
/// thread never allocates; stages a recipe does not use cost nothing.
pub struct Voice {
    sample_rate: f32,
    recipe: Recipe,
    tone: f32,
    volume: f32,
    shifters: [PitchShifter; 3],
    vibrato: Lfo,
    wobble: Wander,
    ring: Lfo,
    tremolo: Lfo,
    noise: Noise,
    vocoder: Vocoder,
    comb: Delay,
    chorus: Modulated,
    flanger: Modulated,
    highpass: Biquad,
    lowpass: Biquad,
    crusher: Crusher,
    envelope: Envelope,
    echo: Echo,
    reverb: Reverb,
}

impl Voice {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        Self {
            sample_rate: sr,
            recipe: Recipe::DRY,
            tone: 0.0,
            volume: 1.0,
            shifters: std::array::from_fn(|_| PitchShifter::new(sample_rate)),
            vibrato: Lfo::default(),
            wobble: Wander::default(),
            ring: Lfo::default(),
            tremolo: Lfo::default(),
            noise: Noise::default(),
            vocoder: Vocoder::new(sr),
            comb: Delay::new((sr * 0.03) as usize),
            chorus: Modulated::new(sr),
            flanger: Modulated::new(sr),
            highpass: Biquad::default(),
            lowpass: Biquad::default(),
            crusher: Crusher::default(),
            envelope: Envelope::default(),
            echo: Echo::new(sr),
            reverb: Reverb::new(sr),
        }
    }

    /// `tone` moves any voice up or down in semitones; `volume` is the output level.
    pub fn set(&mut self, recipe: &Recipe, tone: f32, volume: f32) {
        if *recipe != self.recipe {
            self.recipe = *recipe;
            let sr = self.sample_rate;
            if recipe.highpass_hz > 0.0 {
                self.highpass = Biquad::new(Shape::High, sr, recipe.highpass_hz, 0.707);
            }
            if recipe.lowpass_hz > 0.0 {
                self.lowpass = Biquad::new(Shape::Low, sr, recipe.lowpass_hz, 0.707);
            }
            // Tails of the previous voice would ring on into the new one.
            self.comb.clear();
            self.chorus.clear();
            self.flanger.clear();
            self.echo.clear();
            self.reverb.clear();
        }
        self.tone = tone;
        self.volume = volume;
    }

    fn shifting(&self) -> bool {
        let r = &self.recipe;
        (r.pitch + self.tone).abs() > 0.01 || r.vibrato_cents > 0.0 || r.wobble_cents > 0.0
    }

    /// How far behind the input the output currently is, in samples.
    pub fn delay(&self) -> f32 {
        if self.shifting() { self.shifters[0].delay } else { 0.0 }
    }

    pub fn process(&mut self, block: &mut [f32]) {
        let r = self.recipe;
        let sr = self.sample_rate;
        if r == Recipe::DRY && self.tone == 0.0 {
            block.iter_mut().for_each(|s| *s *= self.volume);
            return;
        }
        let shifting = self.shifting();
        let modulated = r.vibrato_cents > 0.0 || r.wobble_cents > 0.0;
        self.shifters[0].set_ratio(ratio(r.pitch + self.tone));
        for (k, semis) in r.harmony.iter().enumerate() {
            self.shifters[k + 1].set_ratio(ratio(semis + self.tone));
        }
        let harmonies = r.harmony.iter().filter(|s| **s != 0.0).count();

        for s in block.iter_mut() {
            let dry = *s;
            let mut x = dry;
            if shifting {
                if modulated {
                    let cents = self.vibrato.next(r.vibrato_hz, sr) * r.vibrato_cents
                        + self.wobble.next(r.wobble_hz, sr) * r.wobble_cents;
                    self.shifters[0].set_ratio(ratio(r.pitch + self.tone + cents / 100.0));
                }
                x = self.shifters[0].process(dry);
            }
            if harmonies > 0 {
                let mut h = 0.0;
                for k in 0..2 {
                    if r.harmony[k] != 0.0 {
                        h += self.shifters[k + 1].process(dry);
                    }
                }
                x = x * (1.0 - r.harmony_mix) + h / harmonies as f32 * r.harmony_mix;
            }
            if r.ring_mix > 0.0 {
                x = x * (1.0 - r.ring_mix) + x * self.ring.next(r.ring_hz, sr) * r.ring_mix * 1.4;
            }
            if r.whisper > 0.0 {
                x = x * (1.0 - r.whisper) + x * self.noise.next() * 1.7 * r.whisper;
            }
            if r.vocoder_hz > 0.0 {
                x = self.vocoder.process(x, r.vocoder_hz, r.vocoder_chord);
            }
            if r.comb_ms > 0.0 {
                let y = x + self.comb.read(r.comb_ms * sr / 1000.0 - 1.0) * r.comb_feedback;
                self.comb.push(y);
                x = y * (1.0 - r.comb_feedback * 0.5);
            }
            if r.chorus > 0.0 {
                x = x * (1.0 - r.chorus * 0.5) + self.chorus.chorus(x) * r.chorus;
            }
            if r.flanger_mix > 0.0 {
                x = x * (1.0 - r.flanger_mix * 0.5) + self.flanger.flanger(x, r.flanger_hz) * r.flanger_mix * 0.6;
            }
            if r.tremolo_depth > 0.0 {
                x *= 1.0 - r.tremolo_depth * (0.5 + 0.5 * self.tremolo.next(r.tremolo_hz, sr));
            }
            if r.highpass_hz > 0.0 {
                x = self.highpass.process(x);
            }
            if r.lowpass_hz > 0.0 {
                x = self.lowpass.process(x);
            }
            if r.drive > 0.0 {
                x = if r.hard_clip { (x * r.drive).clamp(-0.6, 0.6) } else { saturate(x, r.drive) * 0.8 };
            }
            if r.crush_bits > 0 {
                x = self.crusher.process(x, r.crush_hz, r.crush_bits, sr);
            }
            if r.hiss > 0.0 {
                let talking = self.envelope.next(dry, 0.01, 0.0005);
                x += self.noise.next() * r.hiss * (talking * 4.0).min(1.0);
            }
            if r.echo_mix > 0.0 {
                x += self.echo.process(x, r.echo_ms, r.echo_feedback) * r.echo_mix;
            }
            if r.reverb_mix > 0.0 {
                x = x * (1.0 - r.reverb_mix * 0.3)
                    + self.reverb.process(x, r.reverb_size, r.reverb_damp) * r.reverb_mix * 2.0;
            }
            // Soft limit so a loud scream never clips hard in voice chat.
            *s = (x * r.gain * self.volume).tanh();
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
}

#[cfg(test)]
mod latency {
    use super::*;

    /// The shifter's own delay is the part of the voice latency we control; keep it small
    /// in both directions. Prints the numbers with `--nocapture`.
    #[test]
    fn pitch_shift_delay_stays_under_10_ms() {
        let sr = 48_000u32;
        // A rough voice: 120 Hz buzz with harmonics.
        let input: Vec<f32> = (0..sr as usize * 2)
            .map(|i| {
                (1..8)
                    .map(|h| (i as f32 * 120.0 * h as f32 * std::f32::consts::TAU / sr as f32).sin() / h as f32)
                    .sum::<f32>()
                    * 0.2
            })
            .collect();
        for semis in [-12.0f32, -6.0, -3.0, 3.0, 8.0, 12.0] {
            let mut p = PitchShifter::new(sr);
            p.set_ratio(2f32.powf(semis / 12.0));
            let mut sum = 0.0f64;
            for (i, s) in input.iter().enumerate() {
                p.process(*s);
                if i > sr as usize / 2 {
                    sum += p.delay as f64;
                }
            }
            let avg = sum / (input.len() - sr as usize / 2 - 1) as f64 / sr as f64 * 1000.0;
            println!("{semis:+} st: average delay {avg:.2} ms");
            assert!(avg < 10.0, "{semis:+} st: {avg:.2} ms");
        }
    }
}
