//! The voice catalog. Every voice is a `Recipe`; names avoid trademarks and describe
//! what people hear. Starting values were tuned by ear and checked by the tests below.

use crate::dsp::{Recipe, Voice};
use crate::lang::tr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Basics,
    People,
    Creatures,
    Robots,
    Places,
    Devices,
    Fun,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Basics,
        Category::People,
        Category::Creatures,
        Category::Robots,
        Category::Places,
        Category::Devices,
        Category::Fun,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::Basics => tr("category.basics"),
            Category::People => tr("category.people"),
            Category::Creatures => tr("category.creatures"),
            Category::Robots => tr("category.robots"),
            Category::Places => tr("category.places"),
            Category::Devices => tr("category.devices"),
            Category::Fun => tr("category.fun"),
        }
    }
}

pub struct VoiceDef {
    /// Stable key for settings files; never rename.
    pub id: &'static str,
    pub category: Category,
    pub recipe: Recipe,
}

impl VoiceDef {
    pub fn name(&self) -> &'static str {
        crate::lang::voice(self.id)
    }
}

const D: Recipe = Recipe::DRY;

macro_rules! voice {
    ($id:literal, $cat:ident, { $($field:ident: $value:expr),* $(,)? }) => {
        VoiceDef { id: $id, category: Category::$cat, recipe: Recipe { $($field: $value,)* ..D } }
    };
}

pub static ALL: &[VoiceDef] = &[
    // Basics: the first six keep the ids the app started with.
    voice!("off", Basics, {}),
    voice!("minion", Basics, { pitch: 8.0 }),
    voice!("helium", Basics, { pitch: 12.0 }),
    voice!("giant", Basics, { pitch: -6.0, reverb_size: 0.3, reverb_damp: 0.5, reverb_mix: 0.12 }),
    voice!("robot", Basics, { ring_hz: 55.0, ring_mix: 1.0, comb_ms: 6.0, comb_feedback: 0.55, gain: 1.4 }),
    voice!("radio", Basics, { highpass_hz: 450.0, lowpass_hz: 2800.0, drive: 4.0, gain: 1.2 }),
    // People
    voice!("kid", People, { pitch: 5.0, chorus: 0.15 }),
    voice!("baby", People, { pitch: 9.0, tremolo_hz: 6.0, tremolo_depth: 0.15 }),
    voice!("woman", People, { pitch: 4.0, highpass_hz: 120.0 }),
    voice!("man", People, { pitch: -3.0 }),
    voice!("bass", People, { pitch: -7.0, lowpass_hz: 6000.0, reverb_size: 0.2, reverb_damp: 0.6, reverb_mix: 0.1 }),
    voice!("grandpa", People, { pitch: -2.0, vibrato_hz: 5.0, vibrato_cents: 18.0, lowpass_hz: 4000.0 }),
    voice!("grandma", People, { pitch: 2.0, vibrato_hz: 5.5, vibrato_cents: 28.0, highpass_hz: 200.0 }),
    voice!("teen", People, { pitch: 2.0, wobble_hz: 2.0, wobble_cents: 45.0 }),
    voice!("pirate", People, { pitch: -4.0, drive: 2.0, reverb_size: 0.25, reverb_damp: 0.5, reverb_mix: 0.12 }),
    voice!("nervous", People, { vibrato_hz: 7.0, vibrato_cents: 35.0, tremolo_hz: 7.0, tremolo_depth: 0.2 }),
    voice!("drunk", People, { pitch: -1.0, wobble_hz: 1.2, wobble_cents: 70.0, echo_ms: 140.0, echo_feedback: 0.2, echo_mix: 0.2 }),
    voice!("whisper", People, { whisper: 1.0, highpass_hz: 300.0, gain: 1.6 }),
    voice!("sleepy", People, { pitch: -2.0, lowpass_hz: 3500.0, tremolo_hz: 0.5, tremolo_depth: 0.25 }),
    voice!("announcer", People, { pitch: -2.0, drive: 1.5, echo_ms: 260.0, echo_feedback: 0.25, echo_mix: 0.2, reverb_size: 0.7, reverb_damp: 0.4, reverb_mix: 0.2 }),
    // Creatures
    voice!("gremlin", Creatures, { pitch: 6.0, ring_hz: 90.0, ring_mix: 0.15, drive: 1.5 }),
    voice!("rodent", Creatures, { pitch: 10.0, chorus: 0.3 }),
    voice!("ogre", Creatures, { pitch: -6.0, drive: 2.5, lowpass_hz: 5000.0 }),
    voice!("troll", Creatures, { pitch: -7.0, ring_hz: 30.0, ring_mix: 0.3, reverb_size: 0.3, reverb_damp: 0.5, reverb_mix: 0.15 }),
    voice!("demon", Creatures, { pitch: -8.0, harmony: [-12.0, 0.0], harmony_mix: 0.45, drive: 2.5, reverb_size: 0.8, reverb_damp: 0.4, reverb_mix: 0.25 }),
    voice!("zombie", Creatures, { pitch: -3.0, vibrato_hz: 3.0, vibrato_cents: 25.0, crush_hz: 12000.0, crush_bits: 8, lowpass_hz: 2500.0, gain: 1.3 }),
    voice!("ghost", Creatures, { pitch: 1.0, whisper: 0.35, flanger_hz: 0.2, flanger_mix: 0.5, reverb_size: 0.75, reverb_damp: 0.35, reverb_mix: 0.4 }),
    voice!("possessed", Creatures, { harmony: [-5.0, 4.0], harmony_mix: 0.5, ring_hz: 60.0, ring_mix: 0.15, reverb_size: 0.8, reverb_damp: 0.5, reverb_mix: 0.3 }),
    voice!("vampire", Creatures, { pitch: -3.0, chorus: 0.3, reverb_size: 0.8, reverb_damp: 0.3, reverb_mix: 0.2 }),
    voice!("dragon", Creatures, { pitch: -10.0, harmony: [-5.0, 0.0], harmony_mix: 0.4, drive: 3.0, reverb_size: 0.9, reverb_damp: 0.5, reverb_mix: 0.25 }),
    voice!("swarm", Creatures, { ring_hz: 110.0, ring_mix: 0.4, tremolo_hz: 20.0, tremolo_depth: 0.4 }),
    voice!("alien", Creatures, { pitch: 3.0, ring_hz: 250.0, ring_mix: 0.3, flanger_hz: 1.5, flanger_mix: 0.4 }),
    // Robots & sci-fi
    voice!("android", Robots, { vocoder_hz: 110.0, gain: 1.4 }),
    voice!("dictator", Robots, { ring_hz: 30.0, ring_mix: 1.0, highpass_hz: 400.0, lowpass_hz: 3400.0, drive: 3.0, hard_clip: true, gain: 1.4 }),
    voice!("cyborg", Robots, { pitch: -4.0, comb_ms: 3.0, comb_feedback: 0.6, reverb_size: 0.3, reverb_damp: 0.4, reverb_mix: 0.12 }),
    voice!("mutant", Robots, { wobble_hz: 2.0, wobble_cents: 300.0, chorus: 0.3 }),
    voice!("8bit", Robots, { crush_hz: 8000.0, crush_bits: 5, ring_hz: 440.0, ring_mix: 0.1, gain: 1.2 }),
    voice!("synth_choir", Robots, { vocoder_hz: 220.0, vocoder_chord: true, reverb_size: 0.6, reverb_damp: 0.4, reverb_mix: 0.2, gain: 1.4 }),
    voice!("hologram", Robots, { ring_hz: 8.0, ring_mix: 0.25, flanger_hz: 0.6, flanger_mix: 0.5, echo_ms: 90.0, echo_feedback: 0.3, echo_mix: 0.25 }),
    voice!("spaceship", Robots, { highpass_hz: 500.0, lowpass_hz: 2500.0, hiss: 0.02, drive: 2.0, comb_ms: 1.5, comb_feedback: 0.3 }),
    // Places
    voice!("cave", Places, { reverb_size: 0.95, reverb_damp: 0.45, reverb_mix: 0.5 }),
    voice!("cathedral", Places, { reverb_size: 1.0, reverb_damp: 0.15, reverb_mix: 0.45 }),
    voice!("bathroom", Places, { reverb_size: 0.3, reverb_damp: 0.05, reverb_mix: 0.4 }),
    voice!("stadium", Places, { echo_ms: 300.0, echo_feedback: 0.25, echo_mix: 0.35, reverb_size: 0.8, reverb_damp: 0.3, reverb_mix: 0.25 }),
    voice!("underwater", Places, { lowpass_hz: 900.0, chorus: 0.5, vibrato_hz: 3.0, vibrato_cents: 25.0, gain: 1.5 }),
    voice!("canyon", Places, { echo_ms: 380.0, echo_feedback: 0.28, echo_mix: 0.5 }),
    voice!("tin_can", Places, { comb_ms: 1.2, comb_feedback: 0.7, highpass_hz: 300.0 }),
    // Devices
    voice!("phone", Devices, { highpass_hz: 300.0, lowpass_hz: 3400.0, drive: 1.5, gain: 1.3 }),
    voice!("megaphone", Devices, { highpass_hz: 500.0, lowpass_hz: 4000.0, drive: 5.0, hard_clip: true, reverb_size: 0.4, reverb_damp: 0.4, reverb_mix: 0.15 }),
    voice!("walkie", Devices, { highpass_hz: 400.0, lowpass_hz: 3000.0, crush_hz: 8000.0, crush_bits: 8, hiss: 0.04, drive: 2.0 }),
    voice!("old_radio", Devices, { highpass_hz: 300.0, lowpass_hz: 3500.0, drive: 2.5, hiss: 0.02, tremolo_hz: 0.5, tremolo_depth: 0.2 }),
    voice!("drive_thru", Devices, { highpass_hz: 600.0, lowpass_hz: 3000.0, drive: 4.0, hiss: 0.03 }),
    voice!("cassette", Devices, { wobble_hz: 0.8, wobble_cents: 20.0, lowpass_hz: 7000.0, hiss: 0.01, drive: 1.3 }),
    // Fun
    voice!("chipmunk_choir", Fun, { pitch: 12.0, harmony: [16.0, 19.0], harmony_mix: 0.5 }),
    voice!("opera", Fun, { vibrato_hz: 5.5, vibrato_cents: 45.0, reverb_size: 0.9, reverb_damp: 0.3, reverb_mix: 0.3 }),
    voice!("choir", Fun, { harmony: [4.0, 7.0], harmony_mix: 0.5, chorus: 0.3, reverb_size: 0.9, reverb_damp: 0.3, reverb_mix: 0.3 }),
    voice!("karaoke", Fun, { echo_ms: 180.0, echo_feedback: 0.3, echo_mix: 0.3, reverb_size: 0.5, reverb_damp: 0.2, reverb_mix: 0.2 }),
    voice!("glitch", Fun, { wobble_hz: 8.0, wobble_cents: 60.0, crush_hz: 11000.0, crush_bits: 10, ring_hz: 15.0, ring_mix: 0.2 }),
    voice!("duet", Fun, { harmony: [-12.0, 0.0], harmony_mix: 0.5 }),
    voice!("jet", Fun, { flanger_hz: 0.25, flanger_mix: 0.9 }),
    voice!("helium_robot", Fun, { pitch: 9.0, ring_hz: 70.0, ring_mix: 0.7, comb_ms: 4.0, comb_feedback: 0.4 }),
];

pub fn index_of(id: &str) -> Option<usize> {
    ALL.iter().position(|v| v.id == id)
}

/// Every voice's own waveform for its icon, in `ALL` order: a buzzy test tone run through
/// the recipe, `points` samples from the last 20 ms, normalised to -1..1.
pub fn glyphs(points: usize) -> Vec<Vec<f32>> {
    const SR: u32 = 48_000;
    // The same tone for every voice, so it is made once: a third of the work at startup.
    let tone: Vec<f32> = (0..SR as usize / 8)
        .map(|i| {
            let t = i as f32 / SR as f32;
            (1..6).map(|h| (t * 140.0 * h as f32 * std::f32::consts::TAU).sin() / h as f32).sum::<f32>() * 0.3
        })
        .collect();
    let mut buf = tone.clone();
    ALL.iter()
        .map(|def| {
            let mut voice = Voice::new(SR);
            voice.set(&def.recipe, 0.0, 1.0);
            buf.copy_from_slice(&tone);
            voice.process(&mut buf);
            let tail = &buf[buf.len() - SR as usize / 50..];
            let peak = tail.iter().fold(1e-6f32, |m, s| m.max(s.abs()));
            (0..points).map(|i| tail[i * tail.len() / points] / peak).collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_plain() {
        for v in ALL {
            assert!(v.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "id {}", v.id);
        }
    }

    use std::f32::consts::TAU;

    const SR: u32 = 48_000;

    fn voiced(secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|i| {
                (1..6).map(|h| (i as f32 * 150.0 * h as f32 * TAU / SR as f32).sin() / h as f32).sum::<f32>() * 0.4
            })
            .collect()
    }

    fn run(def: &VoiceDef, input: &[f32]) -> Vec<f32> {
        let mut v = Voice::new(SR);
        v.set(&def.recipe, 0.0, 1.0);
        let mut out = input.to_vec();
        for block in out.chunks_mut(480) {
            v.process(block);
        }
        out
    }

    #[test]
    fn ids_are_unique_and_the_original_six_come_first() {
        for (i, a) in ALL.iter().enumerate() {
            assert!(ALL[i + 1..].iter().all(|b| b.id != a.id), "duplicate id {}", a.id);
        }
        let first: Vec<_> = ALL[..6].iter().map(|v| v.id).collect();
        assert_eq!(first, ["off", "minion", "helium", "giant", "robot", "radio"]);
    }

    #[test]
    fn there_are_plenty_of_voices_in_every_category() {
        assert!(ALL.len() >= 50, "{} voices", ALL.len());
        for c in Category::ALL {
            assert!(ALL.iter().filter(|v| v.category == c).count() >= 6 || c == Category::Basics, "{c:?}");
        }
    }

    #[test]
    fn every_voice_stays_finite_bounded_and_audible() {
        let input = voiced(1.0);
        for def in ALL {
            let out = run(def, &input);
            assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0), "{} blew up", def.id);
            let tail = &out[SR as usize / 2..];
            let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
            assert!(rms > 0.02, "{} is nearly silent: rms {rms}", def.id);
        }
    }

    #[test]
    fn every_voice_goes_quiet_after_the_speaker_does() {
        // A second of voice, then two seconds of silence: no voice may keep ringing or hissing.
        let mut input = voiced(1.0);
        input.extend(std::iter::repeat_n(0.0, SR as usize * 2));
        for def in ALL {
            let out = run(def, &input);
            let last = &out[out.len() - SR as usize / 2..];
            let peak = last.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak < 0.01, "{} still sounds after 1.5 s of silence: {peak}", def.id);
        }
    }

    #[test]
    fn off_is_untouched() {
        let input = voiced(0.2);
        assert_eq!(run(&ALL[0], &input), input);
    }

    #[test]
    fn radio_cuts_lows() {
        let low: Vec<f32> = (0..SR as usize / 2).map(|i| (i as f32 * 80.0 * TAU / SR as f32).sin() * 0.5).collect();
        let out = run(&ALL[index_of("radio").unwrap()], &low);
        let peak = out[SR as usize / 10..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak < 0.15, "80 Hz leaked at {peak}");
    }

    #[test]
    fn glyphs_are_normalised() {
        for (def, g) in ALL.iter().zip(glyphs(64)) {
            assert_eq!(g.len(), 64);
            assert!(g.iter().all(|s| s.abs() <= 1.0 + 1e-6), "{}", def.id);
        }
    }

    /// `cargo test --release realtime_factor -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark"]
    fn realtime_factor() {
        let input = voiced(10.0);
        let mut worst = (0.0f64, "");
        for def in ALL {
            let t = std::time::Instant::now();
            run(def, &input);
            let share = t.elapsed().as_secs_f64() / 10.0 * 100.0;
            println!("{:<16} {share:.3} % of one core", def.id);
            if share > worst.0 {
                worst = (share, def.id);
            }
        }
        println!("heaviest: {} at {:.3} %", worst.1, worst.0);
    }

    /// The same between phrases: a second of voice, then nine of quiet room noise that the
    /// gate shuts out, on a thread set up like the audio one.
    /// `cargo test --release gated_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark"]
    fn gated_cost() {
        crate::dsp::flush_denormals();
        let mut noise = crate::fx::Noise::default();
        let mut input = voiced(1.0);
        input.extend((0..SR * 9).map(|_| noise.next() * 0.0003));
        let mut worst = (0.0f64, "");
        for def in ALL {
            let (mut mic, mut voice) = (crate::mic::Mic::new(SR), Voice::new(SR));
            voice.set(&def.recipe, 0.0, 1.0);
            let mut out = input.clone();
            let t = std::time::Instant::now();
            for block in out.chunks_mut(480) {
                mic.process(block, 0.0, Some(-40.0));
                voice.process(block);
            }
            let share = t.elapsed().as_secs_f64() / 10.0 * 100.0;
            println!("{:<16} {share:.3} % of one core", def.id);
            if share > worst.0 {
                worst = (share, def.id);
            }
        }
        println!("heaviest: {} at {:.3} %", worst.1, worst.0);
    }
}
