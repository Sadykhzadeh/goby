//! Remembers the last devices and voice in %APPDATA%\Goby\settings.ini (plain key=value).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub input: String,
    pub output: String,
    /// Voice id from the catalog; ids are stable across versions.
    pub voice: String,
    pub semitones: f32,
    pub volume: f32,
    pub monitor: bool,
    /// Effect on; off sends your own voice (still gained and gated).
    pub enabled: bool,
    pub mic_gain_db: f32,
    pub gate: bool,
    pub gate_db: f32,
    /// Share of the original voice mixed back in, 0..1.
    pub dry_mix: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            voice: "minion".into(),
            semitones: 0.0,
            volume: 1.0,
            monitor: false,
            enabled: true,
            mic_gain_db: 0.0,
            gate: false,
            gate_db: -50.0,
            dry_mix: 0.0,
        }
    }
}

fn path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Goby").join("settings.ini"))
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            let v = v.trim();
            match k.trim() {
                "input" => s.input = v.to_string(),
                "output" => s.output = v.to_string(),
                "voice" => s.voice = v.to_string(),
                "semitones" => s.semitones = v.parse::<f32>().unwrap_or(s.semitones).clamp(-12.0, 12.0),
                "volume" => s.volume = v.parse::<f32>().unwrap_or(s.volume).clamp(0.0, 2.0),
                "monitor" => s.monitor = v == "1",
                "enabled" => s.enabled = v != "0",
                "mic_gain_db" => s.mic_gain_db = v.parse::<f32>().unwrap_or(s.mic_gain_db).clamp(-12.0, 24.0),
                "gate" => s.gate = v == "1",
                "gate_db" => s.gate_db = v.parse::<f32>().unwrap_or(s.gate_db).clamp(-80.0, -10.0),
                "dry_mix" => s.dry_mix = v.parse::<f32>().unwrap_or(s.dry_mix).clamp(0.0, 1.0),
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        format!(
            "input={}\noutput={}\nvoice={}\nsemitones={}\nvolume={}\nmonitor={}\nenabled={}\nmic_gain_db={}\ngate={}\ngate_db={}\ndry_mix={}\n",
            self.input,
            self.output,
            self.voice,
            self.semitones,
            self.volume,
            self.monitor as u8,
            self.enabled as u8,
            self.mic_gain_db,
            self.gate as u8,
            self.gate_db,
            self.dry_mix,
        )
    }

    pub fn load() -> Settings {
        path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, self.render());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let s = Settings {
            input: "{0.0.1.00000000}.{abc}".into(),
            output: "{0.0.0.00000000}.{def}".into(),
            voice: "demon".into(),
            semitones: -6.0,
            volume: 1.25,
            monitor: true,
            enabled: false,
            mic_gain_db: 6.0,
            gate: true,
            gate_db: -45.0,
            dry_mix: 0.25,
        };
        assert_eq!(Settings::parse(&s.render()), s);
    }

    #[test]
    fn junk_falls_back_to_defaults() {
        let s = Settings::parse("semitones=banana\nvolume=99\nnoise");
        assert_eq!(s.semitones, Settings::default().semitones);
        assert_eq!(s.volume, 2.0);
    }
}
