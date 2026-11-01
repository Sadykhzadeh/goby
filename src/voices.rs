//! The voice catalog. Every voice is a `Recipe`; names avoid trademarks and describe
//! what people hear. Starting values were tuned by ear and checked by the tests below.

use crate::dsp::{Recipe, Voice};

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

    pub fn name(self, ru: bool) -> &'static str {
        match (self, ru) {
            (Category::Basics, true) => "Основные",
            (Category::Basics, false) => "Basics",
            (Category::People, true) => "Люди",
            (Category::People, false) => "People",
            (Category::Creatures, true) => "Существа",
            (Category::Creatures, false) => "Creatures",
            (Category::Robots, true) => "Роботы",
            (Category::Robots, false) => "Robots",
            (Category::Places, true) => "Места",
            (Category::Places, false) => "Places",
            (Category::Devices, true) => "Устройства",
            (Category::Devices, false) => "Devices",
            (Category::Fun, true) => "Приколы",
            (Category::Fun, false) => "Fun",
        }
    }
}

pub struct VoiceDef {
    /// Stable key for settings files; never rename.
    pub id: &'static str,
    pub ru: &'static str,
    pub en: &'static str,
    pub category: Category,
    pub recipe: Recipe,
}

impl VoiceDef {
    pub fn name(&self, ru: bool) -> &'static str {
        if ru { self.ru } else { self.en }
    }
}

const D: Recipe = Recipe::DRY;

macro_rules! voice {
    ($id:literal, $ru:literal, $en:literal, $cat:ident, { $($field:ident: $value:expr),* $(,)? }) => {
        VoiceDef { id: $id, ru: $ru, en: $en, category: Category::$cat, recipe: Recipe { $($field: $value,)* ..D } }
    };
}

pub static ALL: &[VoiceDef] = &[
    // Basics: the first six keep the ids the app started with.
    voice!("off", "Без эффекта", "Off", Basics, {}),
    voice!("minion", "Миньон", "Minion", Basics, { pitch: 8.0 }),
    voice!("helium", "Гелий", "Helium", Basics, { pitch: 12.0 }),
    voice!("giant", "Великан", "Giant", Basics, { pitch: -6.0, reverb_size: 0.3, reverb_damp: 0.5, reverb_mix: 0.12 }),
    voice!("robot", "Робот", "Robot", Basics, { ring_hz: 55.0, ring_mix: 1.0, comb_ms: 6.0, comb_feedback: 0.55, gain: 1.4 }),
    voice!("radio", "Рация", "Radio", Basics, { highpass_hz: 450.0, lowpass_hz: 2800.0, drive: 4.0, gain: 1.2 }),
];

pub fn index_of(id: &str) -> Option<usize> {
    ALL.iter().position(|v| v.id == id)
}

/// The voice's own waveform for its icon: a buzzy test tone run through the recipe,
/// `points` samples from the last 20 ms, normalised to -1..1.
pub fn glyph(def: &VoiceDef, points: usize) -> Vec<f32> {
    const SR: u32 = 48_000;
    let mut voice = Voice::new(SR);
    voice.set(&def.recipe, 0.0, 1.0);
    let mut buf: Vec<f32> = (0..SR as usize / 8)
        .map(|i| {
            let t = i as f32 / SR as f32;
            (1..6).map(|h| (t * 140.0 * h as f32 * std::f32::consts::TAU).sin() / h as f32).sum::<f32>() * 0.3
        })
        .collect();
    voice.process(&mut buf);
    let tail = &buf[buf.len() - SR as usize / 50..];
    let peak = tail.iter().fold(1e-6f32, |m, s| m.max(s.abs()));
    (0..points).map(|i| tail[i * tail.len() / points] / peak).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        for def in ALL {
            let g = glyph(def, 64);
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
}
