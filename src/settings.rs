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
    /// Take the mic in exclusive mode for shorter packets (falls back if refused).
    pub exclusive: bool,
    /// Programs (lower-case exe names) whose microphone is pinned to Goby.
    pub routed: Vec<String>,
    /// Programs to hand back to the default mic the next time they run.
    pub unroute: Vec<String>,
    /// The cable was offered a rename once; never prompt for it again.
    pub cable_named_tried: bool,
    /// Closing the window leaves Goby running in the notification area.
    pub tray: bool,
    /// The first close to the tray said where Goby went; it says so only once.
    pub tray_told: bool,
    /// Ctrl+Alt+G and Ctrl+Alt+Page Up/Down work from inside other programs.
    pub hotkeys: bool,
    /// Starred voice ids, in the order they were starred.
    pub favorites: Vec<String>,
    /// Voices saved with their own pitch, volume and mix, in the order they were saved.
    pub presets: Vec<Preset>,
    /// Look for a newer signed release once a day.
    pub updates: bool,
    /// When that was last done, in seconds since 1970.
    pub update_checked: u64,
}

/// A voice as someone tuned it, saved under the name of the voice and its changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub voice: String,
    pub semitones: f32,
    pub dry_mix: f32,
    pub volume: f32,
}

impl Preset {
    /// Whether the voice is set exactly like this now.
    pub fn matches(&self, voice: &str, semitones: f32, dry_mix: f32, volume: f32) -> bool {
        let near = |a: f32, b: f32| (a - b).abs() < 0.005;
        self.voice == voice
            && near(self.semitones, semitones)
            && near(self.dry_mix, dry_mix)
            && near(self.volume, volume)
    }
}

/// `voice:semitones:mix:volume` entries separated by `;`; malformed ones are dropped.
fn presets(v: &str) -> Vec<Preset> {
    v.split(';')
        .filter_map(|entry| {
            let mut parts = entry.trim().split(':');
            let voice = parts.next()?.trim().to_ascii_lowercase();
            let mut number = || parts.next()?.trim().parse::<f32>().ok().filter(|x| x.is_finite());
            let (semitones, dry_mix, volume) = (number()?, number()?, number()?);
            (!voice.is_empty()).then(|| Preset {
                voice,
                semitones: semitones.clamp(-12.0, 12.0),
                dry_mix: dry_mix.clamp(0.0, 1.0),
                volume: volume.clamp(0.0, 2.0),
            })
        })
        .collect()
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
            exclusive: true,
            routed: Vec::new(),
            unroute: Vec::new(),
            cable_named_tried: false,
            tray: true,
            tray_told: false,
            hotkeys: true,
            favorites: Vec::new(),
            presets: Vec::new(),
            updates: true,
            update_checked: 0,
        }
    }
}

/// Comma-separated executable names, lower-cased, empty entries dropped.
fn list(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()
}

fn path() -> Option<PathBuf> {
    Settings::dir().map(|d| d.join("settings.ini"))
}

impl Settings {
    /// `%APPDATA%\Goby`, where the settings live.
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Goby"))
    }

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
                "exclusive" => s.exclusive = v != "0",
                "routed" => s.routed = list(v),
                "unroute" => s.unroute = list(v),
                "cable_named_tried" => s.cable_named_tried = v == "1",
                "tray" => s.tray = v != "0",
                "tray_told" => s.tray_told = v == "1",
                "hotkeys" => s.hotkeys = v != "0",
                "favorites" => s.favorites = list(v),
                "presets" => s.presets = presets(v),
                "updates" => s.updates = v != "0",
                "update_checked" => s.update_checked = v.parse().unwrap_or(0),
                "dry_mix" => s.dry_mix = v.parse::<f32>().unwrap_or(s.dry_mix).clamp(0.0, 1.0),
                _ => {}
            }
        }
        // Files from before the voice catalog stored an absolute pitch per preset; as an
        // offset on top of a voice that would double it, so start those from zero.
        if !text.lines().any(|l| l.trim_start().starts_with("voice=")) {
            s.semitones = 0.0;
        }
        s
    }

    pub fn render(&self) -> String {
        format!(
            "input={}\noutput={}\nvoice={}\nsemitones={}\nvolume={}\nmonitor={}\nenabled={}\nmic_gain_db={}\ngate={}\ngate_db={}\ndry_mix={}\nexclusive={}\nrouted={}\nunroute={}\ncable_named_tried={}\ntray={}\ntray_told={}\nhotkeys={}\nfavorites={}\npresets={}\nupdates={}\nupdate_checked={}\n",
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
            self.exclusive as u8,
            self.routed.join(","),
            self.unroute.join(","),
            self.cable_named_tried as u8,
            self.tray as u8,
            self.tray_told as u8,
            self.hotkeys as u8,
            self.favorites.join(","),
            self.presets
                .iter()
                .map(|p| format!("{}:{}:{}:{}", p.voice, p.semitones, p.dry_mix, p.volume))
                .collect::<Vec<_>>()
                .join(";"),
            self.updates as u8,
            self.update_checked,
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
            // Write then rename, so a crash mid-write never leaves a half file behind.
            let tmp = p.with_extension("ini.tmp");
            if std::fs::write(&tmp, self.render()).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
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
            exclusive: false,
            routed: vec!["cs2.exe".into(), "discord.exe".into()],
            unroute: vec!["obs64.exe".into()],
            cable_named_tried: true,
            tray: false,
            tray_told: true,
            hotkeys: false,
            favorites: vec!["demon".into(), "robot".into()],
            presets: vec![
                Preset { voice: "demon".into(), semitones: 3.0, dry_mix: 0.2, volume: 1.0 },
                Preset { voice: "robot".into(), semitones: -2.5, dry_mix: 0.0, volume: 1.25 },
            ],
            updates: false,
            update_checked: 1_794_000_000,
        };
        assert_eq!(Settings::parse(&s.render()), s);
    }

    #[test]
    fn old_files_do_not_double_the_pitch() {
        let s = Settings::parse("preset=1\nsemitones=8\nvolume=1\n");
        assert_eq!(s.semitones, 0.0);
        assert_eq!(s.voice, "minion");
    }

    #[test]
    fn broken_presets_are_dropped_and_wild_values_clamped() {
        let s = Settings::parse("presets=demon:3:0.2:1;robot;:1:0:1;ghost:x:0:1;minion:40:-1:9;giant:1:0:NaN\n");
        assert_eq!(
            s.presets,
            vec![
                Preset { voice: "demon".into(), semitones: 3.0, dry_mix: 0.2, volume: 1.0 },
                Preset { voice: "minion".into(), semitones: 12.0, dry_mix: 0.0, volume: 2.0 },
            ]
        );
        assert!(s.presets[0].matches("demon", 3.001, 0.2, 1.0));
        assert!(!s.presets[0].matches("demon", 3.0, 0.3, 1.0));
    }

    #[test]
    fn junk_falls_back_to_defaults() {
        let s = Settings::parse("semitones=banana\nvolume=99\nnoise");
        assert_eq!(s.semitones, Settings::default().semitones);
        assert_eq!(s.volume, 2.0);
    }
}
