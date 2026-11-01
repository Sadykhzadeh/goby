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
    // People
    voice!("kid", "Ребёнок", "Kid", People, { pitch: 5.0, chorus: 0.15 }),
    voice!("baby", "Младенец", "Baby", People, { pitch: 9.0, tremolo_hz: 6.0, tremolo_depth: 0.15 }),
    voice!("woman", "Женский", "Woman", People, { pitch: 4.0, highpass_hz: 120.0 }),
    voice!("man", "Мужской", "Man", People, { pitch: -3.0 }),
    voice!("bass", "Бас", "Deep voice", People, { pitch: -7.0, lowpass_hz: 6000.0, reverb_size: 0.2, reverb_damp: 0.6, reverb_mix: 0.1 }),
    voice!("grandpa", "Дедушка", "Grandpa", People, { pitch: -2.0, vibrato_hz: 5.0, vibrato_cents: 18.0, lowpass_hz: 4000.0 }),
    voice!("grandma", "Бабушка", "Grandma", People, { pitch: 2.0, vibrato_hz: 5.5, vibrato_cents: 28.0, highpass_hz: 200.0 }),
    voice!("teen", "Подросток", "Teen", People, { pitch: 2.0, wobble_hz: 2.0, wobble_cents: 45.0 }),
    voice!("pirate", "Пират", "Pirate", People, { pitch: -4.0, drive: 2.0, reverb_size: 0.25, reverb_damp: 0.5, reverb_mix: 0.12 }),
    voice!("nervous", "Волнуется", "Nervous", People, { vibrato_hz: 7.0, vibrato_cents: 35.0, tremolo_hz: 7.0, tremolo_depth: 0.2 }),
    voice!("drunk", "Пьяный", "Tipsy", People, { pitch: -1.0, wobble_hz: 1.2, wobble_cents: 70.0, echo_ms: 140.0, echo_feedback: 0.2, echo_mix: 0.2 }),
    voice!("whisper", "Шёпот", "Whisper", People, { whisper: 1.0, highpass_hz: 300.0, gain: 1.6 }),
    voice!("sleepy", "Сонный", "Sleepy", People, { pitch: -2.0, lowpass_hz: 3500.0, tremolo_hz: 0.5, tremolo_depth: 0.25 }),
    voice!("announcer", "Ведущий", "Announcer", People, { pitch: -2.0, drive: 1.5, echo_ms: 260.0, echo_feedback: 0.25, echo_mix: 0.2, reverb_size: 0.7, reverb_damp: 0.4, reverb_mix: 0.2 }),
    // Creatures
    voice!("gremlin", "Гремлин", "Gremlin", Creatures, { pitch: 6.0, ring_hz: 90.0, ring_mix: 0.15, drive: 1.5 }),
    voice!("rodent", "Грызун", "Rodent", Creatures, { pitch: 10.0, chorus: 0.3 }),
    voice!("ogre", "Огр", "Ogre", Creatures, { pitch: -6.0, drive: 2.5, lowpass_hz: 5000.0 }),
    voice!("troll", "Тролль", "Troll", Creatures, { pitch: -7.0, ring_hz: 30.0, ring_mix: 0.3, reverb_size: 0.3, reverb_damp: 0.5, reverb_mix: 0.15 }),
    voice!("demon", "Демон", "Demon", Creatures, { pitch: -8.0, harmony: [-12.0, 0.0], harmony_mix: 0.45, drive: 2.5, reverb_size: 0.8, reverb_damp: 0.4, reverb_mix: 0.25 }),
    voice!("zombie", "Зомби", "Zombie", Creatures, { pitch: -3.0, vibrato_hz: 3.0, vibrato_cents: 25.0, crush_hz: 12000.0, crush_bits: 8, lowpass_hz: 2500.0, gain: 1.3 }),
    voice!("ghost", "Привидение", "Ghost", Creatures, { pitch: 1.0, whisper: 0.35, flanger_hz: 0.2, flanger_mix: 0.5, reverb_size: 0.75, reverb_damp: 0.35, reverb_mix: 0.4 }),
    voice!("possessed", "Одержимый", "Possessed", Creatures, { harmony: [-5.0, 4.0], harmony_mix: 0.5, ring_hz: 60.0, ring_mix: 0.15, reverb_size: 0.8, reverb_damp: 0.5, reverb_mix: 0.3 }),
    voice!("vampire", "Вампир", "Vampire", Creatures, { pitch: -3.0, chorus: 0.3, reverb_size: 0.8, reverb_damp: 0.3, reverb_mix: 0.2 }),
    voice!("dragon", "Дракон", "Dragon", Creatures, { pitch: -10.0, harmony: [-5.0, 0.0], harmony_mix: 0.4, drive: 3.0, reverb_size: 0.9, reverb_damp: 0.5, reverb_mix: 0.25 }),
    voice!("swarm", "Рой", "Swarm", Creatures, { ring_hz: 110.0, ring_mix: 0.4, tremolo_hz: 20.0, tremolo_depth: 0.4 }),
    voice!("alien", "Пришелец", "Alien", Creatures, { pitch: 3.0, ring_hz: 250.0, ring_mix: 0.3, flanger_hz: 1.5, flanger_mix: 0.4 }),
    // Robots & sci-fi
    voice!("android", "Андроид", "Android", Robots, { vocoder_hz: 110.0, gain: 1.4 }),
    voice!("dictator", "Диктатор", "Dictator", Robots, { ring_hz: 30.0, ring_mix: 1.0, highpass_hz: 400.0, lowpass_hz: 3400.0, drive: 3.0, hard_clip: true, gain: 1.4 }),
    voice!("cyborg", "Киборг", "Cyborg", Robots, { pitch: -4.0, comb_ms: 3.0, comb_feedback: 0.6, reverb_size: 0.3, reverb_damp: 0.4, reverb_mix: 0.12 }),
    voice!("mutant", "Мутант", "Mutant", Robots, { wobble_hz: 2.0, wobble_cents: 300.0, chorus: 0.3 }),
    voice!("8bit", "8-бит", "8-bit", Robots, { crush_hz: 8000.0, crush_bits: 5, ring_hz: 440.0, ring_mix: 0.1, gain: 1.2 }),
    voice!("synth_choir", "Синт-хор", "Synth choir", Robots, { vocoder_hz: 220.0, vocoder_chord: true, reverb_size: 0.6, reverb_damp: 0.4, reverb_mix: 0.2, gain: 1.4 }),
    voice!("hologram", "Голограмма", "Hologram", Robots, { ring_hz: 8.0, ring_mix: 0.25, flanger_hz: 0.6, flanger_mix: 0.5, echo_ms: 90.0, echo_feedback: 0.3, echo_mix: 0.25 }),
    voice!("spaceship", "Борт корабля", "Spaceship", Robots, { highpass_hz: 500.0, lowpass_hz: 2500.0, hiss: 0.02, drive: 2.0, comb_ms: 1.5, comb_feedback: 0.3 }),
    // Places
    voice!("cave", "Пещера", "Cave", Places, { reverb_size: 0.95, reverb_damp: 0.45, reverb_mix: 0.5 }),
    voice!("cathedral", "Собор", "Cathedral", Places, { reverb_size: 1.0, reverb_damp: 0.15, reverb_mix: 0.45 }),
    voice!("bathroom", "Ванная", "Bathroom", Places, { reverb_size: 0.3, reverb_damp: 0.05, reverb_mix: 0.4 }),
    voice!("stadium", "Стадион", "Stadium", Places, { echo_ms: 300.0, echo_feedback: 0.25, echo_mix: 0.35, reverb_size: 0.8, reverb_damp: 0.3, reverb_mix: 0.25 }),
    voice!("underwater", "Под водой", "Underwater", Places, { lowpass_hz: 900.0, chorus: 0.5, vibrato_hz: 3.0, vibrato_cents: 25.0, gain: 1.5 }),
    voice!("canyon", "Каньон", "Canyon", Places, { echo_ms: 380.0, echo_feedback: 0.28, echo_mix: 0.5 }),
    voice!("tin_can", "Консервная банка", "Tin can", Places, { comb_ms: 1.2, comb_feedback: 0.7, highpass_hz: 300.0 }),
    // Devices
    voice!("phone", "Телефон", "Phone", Devices, { highpass_hz: 300.0, lowpass_hz: 3400.0, drive: 1.5, gain: 1.3 }),
    voice!("megaphone", "Мегафон", "Megaphone", Devices, { highpass_hz: 500.0, lowpass_hz: 4000.0, drive: 5.0, hard_clip: true, reverb_size: 0.4, reverb_damp: 0.4, reverb_mix: 0.15 }),
    voice!("walkie", "Рация с помехами", "Walkie-talkie", Devices, { highpass_hz: 400.0, lowpass_hz: 3000.0, crush_hz: 8000.0, crush_bits: 8, hiss: 0.04, drive: 2.0 }),
    voice!("old_radio", "Старое радио", "Old radio", Devices, { highpass_hz: 300.0, lowpass_hz: 3500.0, drive: 2.5, hiss: 0.02, tremolo_hz: 0.5, tremolo_depth: 0.2 }),
    voice!("drive_thru", "Окно заказа", "Drive-thru", Devices, { highpass_hz: 600.0, lowpass_hz: 3000.0, drive: 4.0, hiss: 0.03 }),
    voice!("cassette", "Кассета", "Cassette", Devices, { wobble_hz: 0.8, wobble_cents: 20.0, lowpass_hz: 7000.0, hiss: 0.01, drive: 1.3 }),
    // Fun
    voice!("chipmunk_choir", "Хор бурундуков", "Squeaky choir", Fun, { pitch: 12.0, harmony: [16.0, 19.0], harmony_mix: 0.5 }),
    voice!("opera", "Опера", "Opera", Fun, { vibrato_hz: 5.5, vibrato_cents: 45.0, reverb_size: 0.9, reverb_damp: 0.3, reverb_mix: 0.3 }),
    voice!("choir", "Хор", "Choir", Fun, { harmony: [4.0, 7.0], harmony_mix: 0.5, chorus: 0.3, reverb_size: 0.9, reverb_damp: 0.3, reverb_mix: 0.3 }),
    voice!("karaoke", "Караоке", "Karaoke", Fun, { echo_ms: 180.0, echo_feedback: 0.3, echo_mix: 0.3, reverb_size: 0.5, reverb_damp: 0.2, reverb_mix: 0.2 }),
    voice!("glitch", "Глюк", "Glitch", Fun, { wobble_hz: 8.0, wobble_cents: 60.0, crush_hz: 11000.0, crush_bits: 10, ring_hz: 15.0, ring_mix: 0.2 }),
    voice!("duet", "Дуэт с басом", "Duet", Fun, { harmony: [-12.0, 0.0], harmony_mix: 0.5 }),
    voice!("jet", "Реактивный", "Jet", Fun, { flanger_hz: 0.25, flanger_mix: 0.9 }),
    voice!("helium_robot", "Робочипмунк", "Squeaky robot", Fun, { pitch: 9.0, ring_hz: 70.0, ring_mix: 0.7, comb_ms: 4.0, comb_feedback: 0.4 }),
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

    #[test]
    fn every_name_exists_in_both_languages() {
        for v in ALL {
            assert!(!v.ru.trim().is_empty() && !v.en.trim().is_empty(), "{}", v.id);
            assert!(v.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "id {}", v.id);
        }
        for c in Category::ALL {
            assert!(!c.name(true).is_empty() && !c.name(false).is_empty());
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
