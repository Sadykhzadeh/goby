//! What happens to the microphone before the voice: gain, a soft limit and a noise gate.

use crate::fx::{Envelope, db};

/// Up to this level the microphone passes unchanged; above it, peaks are eased towards full
/// scale. A boost can push speech past 1.0, and an unlimited sample reaches the program
/// that receives the cable, which clips it hard.
const KNEE: f32 = 0.8;

/// Identity up to `knee`, then a curve that meets it with the same slope and levels off at
/// 1.0. `x` is finite.
pub fn limit_at(x: f32, knee: f32) -> f32 {
    let a = x.abs();
    if a <= knee {
        return x;
    }
    (knee + (1.0 - knee) * ((a - knee) / (1.0 - knee)).tanh()).copysign(x)
}

fn limit(x: f32) -> f32 {
    limit_at(x, KNEE)
}

/// Input stage: make-up gain, a soft limit, then a gate that closes on keyboard clicks, fans
/// and breathing between phrases.
pub struct Mic {
    sample_rate: f32,
    envelope: Envelope,
    /// Current gate gain, 0 (closed) to 1 (open), smoothed so it never clicks.
    open: f32,
    /// Samples left before a gate that fell below the threshold starts closing.
    hold: u32,
    /// The make-up gain as it is now: it follows the setting with a 10 ms time constant, so a gain
    /// changed between two packets does not jump the waveform.
    gain: f32,
}

impl Mic {
    pub fn new(sample_rate: u32) -> Self {
        Self { sample_rate: sample_rate as f32, envelope: Envelope::default(), open: 1.0, hold: 0, gain: 1.0 }
    }

    /// `gate_db` is the threshold in dBFS; `None` leaves the gate open.
    pub fn process(&mut self, block: &mut [f32], gain_db: f32, gate_db: Option<f32>) {
        let wanted = db(gain_db);
        let threshold = gate_db.map(db);
        // Fast attack so the first syllable is not cut, slow release so words do not chop.
        let (attack, release) =
            (1.0 - (-1.0 / (0.001 * self.sample_rate)).exp(), 1.0 - (-1.0 / (0.08 * self.sample_rate)).exp());
        let ramp = 1.0 - (-1.0 / (0.01 * self.sample_rate)).exp();
        let hold = (0.15 * self.sample_rate) as u32;
        for s in block.iter_mut() {
            self.gain += (wanted - self.gain) * ramp;
            let x = *s * self.gain;
            // A NaN or infinity from the driver would never leave the envelope or the voice's filters.
            let x = if x.is_finite() { limit(x) } else { 0.0 };
            let level = self.envelope.next(x, 0.2, 0.0015);
            let target = match threshold {
                None => 1.0,
                Some(t) if level >= t => {
                    self.hold = hold;
                    1.0
                }
                Some(_) if self.hold > 0 => {
                    self.hold -= 1;
                    1.0
                }
                Some(_) => 0.0,
            };
            self.open += (target - self.open) * if target > self.open { attack } else { release };
            *s = x * self.open;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::sine;

    const SR: u32 = 48_000;

    fn level(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn gain_is_in_decibels() {
        let mut m = Mic::new(SR);
        let mut block = vec![0.1f32; SR as usize / 10];
        m.process(&mut block, 6.0, None);
        assert!((block[block.len() - 1] - 0.1995).abs() < 1e-3, "{}", block[block.len() - 1]);
    }

    /// A gain change is ramped with a 10 ms time constant, so a knob turned while sound plays
    /// does not jump the waveform: around the change no step is more than the louder of the two
    /// levels allows.
    #[test]
    fn a_gain_change_is_ramped_not_stepped() {
        let tone: Vec<f32> = (0..SR as usize / 5).map(|i| sine(220.0, i, SR as f32) * 0.3).collect();
        let largest = |x: &[f32]| (4850..4900).map(|n| (x[n] - x[n - 1]).abs()).fold(0.0f32, f32::max);
        let steady = |gain: f32| {
            let mut x = tone.clone();
            Mic::new(SR).process(&mut x, gain, None);
            largest(&x)
        };
        let mut x = tone.clone();
        let mut m = Mic::new(SR);
        m.process(&mut x[..4855], 0.0, None);
        m.process(&mut x[4855..], 6.0, None);
        let bound = 1.1 * steady(0.0).max(steady(6.0)) + 0.002;
        assert!(largest(&x) <= bound, "a gain change steps the output to {}, over {bound}", largest(&x));
    }

    #[test]
    fn a_boost_past_full_scale_is_eased_in_under_it() {
        assert_eq!(limit(0.79), 0.79);
        assert_eq!(limit(-0.8), -0.8);
        assert!((limit(0.8 + 1e-4) - (0.8 + 1e-4)).abs() < 1e-6);
        let mut last = 0.8f32;
        for step in 1..=500 {
            let y = limit(0.8 + step as f32 * 0.001);
            assert!(y > last && y < 1.0, "{step}: {y}");
            last = y;
        }
        assert_eq!(limit(-f32::MAX), -limit(f32::MAX));
        assert!(limit(f32::MAX) <= 1.0);

        let mut m = Mic::new(SR);
        let mut loud: Vec<f32> = (0..SR as usize).map(|i| sine(200.0, i, SR as f32) * 0.5).collect();
        m.process(&mut loud, 24.0, None);
        assert!(level(&loud) <= 1.0, "boosted speech goes past full scale: {}", level(&loud));
        assert!(level(&loud[SR as usize / 2..]) > 0.9, "boosted speech is too quiet");
    }

    #[test]
    fn gate_mutes_quiet_noise_and_passes_speech() {
        let mut m = Mic::new(SR);
        let mut hum: Vec<f32> = (0..SR as usize).map(|i| sine(50.0, i, SR as f32) * 0.003).collect();
        m.process(&mut hum, 0.0, Some(-40.0));
        assert!(level(&hum[SR as usize / 2..]) < 1e-4, "hum leaks: {}", level(&hum[SR as usize / 2..]));

        let mut speech: Vec<f32> = (0..SR as usize / 2).map(|i| sine(200.0, i, SR as f32) * 0.3).collect();
        m.process(&mut speech, 0.0, Some(-40.0));
        assert!(level(&speech[SR as usize / 100..]) > 0.29, "speech was gated");
    }

    #[test]
    fn a_broken_sample_does_not_outlast_itself() {
        for gate in [None, Some(-40.0)] {
            let mut m = Mic::new(SR);
            let mut x: Vec<f32> = (0..SR as usize).map(|i| sine(200.0, i, SR as f32) * 0.3).collect();
            (x[1000], x[2000]) = (f32::NAN, f32::INFINITY);
            m.process(&mut x, 0.0, gate);
            assert!(x.iter().all(|s| s.is_finite()), "gate {gate:?}: the output is not finite");
            assert_eq!((x[1000], x[2000]), (0.0, 0.0));
            assert!(level(&x[SR as usize / 2..]) > 0.29, "gate {gate:?}: {}", level(&x[SR as usize / 2..]));
        }
    }

    #[test]
    fn gate_holds_between_words() {
        let mut m = Mic::new(SR);
        let mut word: Vec<f32> = (0..SR as usize / 4).map(|i| sine(200.0, i, SR as f32) * 0.3).collect();
        m.process(&mut word, 0.0, Some(-40.0));
        // A 100 ms pause is shorter than the hold, so the next word starts at full level.
        let mut pause = vec![0.0f32; SR as usize / 10];
        m.process(&mut pause, 0.0, Some(-40.0));
        let mut next = vec![0.3f32; 10];
        m.process(&mut next, 0.0, Some(-40.0));
        assert!(next[0] > 0.29, "first sample after a short pause: {}", next[0]);
    }
}
