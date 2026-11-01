//! Remembers the last devices and voice in %APPDATA%\Goby\settings.ini (plain key=value).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub input: String,
    pub output: String,
    pub preset: usize,
    pub semitones: f32,
    pub volume: f32,
    pub monitor: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { input: String::new(), output: String::new(), preset: 1, semitones: 8.0, volume: 1.0, monitor: false }
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
                "preset" => s.preset = v.parse().unwrap_or(s.preset),
                "semitones" => s.semitones = v.parse::<f32>().unwrap_or(s.semitones).clamp(-12.0, 12.0),
                "volume" => s.volume = v.parse::<f32>().unwrap_or(s.volume).clamp(0.0, 2.0),
                "monitor" => s.monitor = v == "1",
                _ => {}
            }
        }
        s
    }

    pub fn render(&self) -> String {
        format!(
            "input={}\noutput={}\npreset={}\nsemitones={}\nvolume={}\nmonitor={}\n",
            self.input, self.output, self.preset, self.semitones, self.volume, self.monitor as u8
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
            preset: 3,
            semitones: -6.0,
            volume: 1.25,
            monitor: true,
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
