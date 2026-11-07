//! Remembers the last devices and voice in %APPDATA%\Goby\settings.ini (plain key=value).

// The website builds this file too; it keeps its settings in the page, not in files.
#[cfg(windows)]
use crate::files;

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
    /// Take the mic in exclusive mode for shorter packets (falls back if refused). Off by
    /// default: while on, no other program can record from that mic. Saved as
    /// `exclusive_mic`; the old `exclusive` key was on by default and is ignored.
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
    /// A language code from `lang::LANGUAGES`, or "auto" for the system's.
    pub lang: String,
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
            let mut number = || decimal(parts.next()?.trim());
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
            exclusive: false,
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
            lang: "auto".into(),
        }
    }
}

/// Comma-separated executable names, lower-cased, empty entries dropped.
fn list(v: &str) -> Vec<String> {
    v.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// A knob's value, or `default` when it is not a finite number. "NaN" parses as a float
/// and comes through `clamp` unchanged, and one NaN gain turns every sample after it to NaN.
fn number(v: &str, default: f32, lo: f32, hi: f32) -> f32 {
    decimal(v).unwrap_or(default).clamp(lo, hi)
}

/// What `str::parse::<u64>` reads: an optional `+`, then decimal digits that fit.
/// The std parser is generic and comes in once per integer type; this is one small copy.
pub fn whole(s: &str) -> Option<u64> {
    let d = s.strip_prefix('+').unwrap_or(s);
    if d.is_empty() {
        return None;
    }
    d.bytes().try_fold(0u64, |n, c| n.checked_mul(10)?.checked_add((c as char).to_digit(10)? as u64))
}

/// What `str::parse::<f32>` reads, kept only when finite: a sign, digits with an optional
/// fraction, an optional exponent. The std parser handles every corner of IEEE rounding and
/// brings 15 KB of tables with it; the numbers in settings.ini are short, and for them one
/// exact division in f64 lands on the same f32 (the tests compare the two widely).
pub fn decimal(s: &str) -> Option<f32> {
    let (neg, s) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() && frac.is_empty() {
        return None;
    }
    // Up to 18 digits are exact in a u64; past that they are far below an f32's precision.
    let (mut m, mut e) = (0u64, 0i32);
    for (i, c) in int.bytes().chain(frac.bytes()).enumerate() {
        let d = (c as char).to_digit(10)? as u64;
        let fraction = i >= int.len();
        if m < 100_000_000_000_000_000 {
            m = m * 10 + d;
            e -= fraction as i32;
        } else {
            e += !fraction as i32;
        }
    }
    if let Some(x) = exp {
        let (eneg, x) = match x.as_bytes().first() {
            Some(b'-') => (true, &x[1..]),
            Some(b'+') => (false, &x[1..]),
            _ => (false, x),
        };
        if x.is_empty() {
            return None;
        }
        // Far past where an f32 overflows or vanishes; saturating keeps the sum in range.
        let n = x.bytes().try_fold(0i32, |n, c| Some((n * 10 + (c as char).to_digit(10)? as i32).min(1000)))?;
        e += if eneg { -n } else { n };
    }
    // Powers of ten up to 1e22 are exact in f64, so for those m / 10^k is one rounding.
    let mut p = 1f64;
    for _ in 0..e.unsigned_abs().min(400) {
        p *= 10.0;
    }
    let v = match m {
        0 => 0.0,
        _ if e < 0 => m as f64 / p,
        _ => m as f64 * p,
    };
    let v = (if neg { -v } else { v }) as f32;
    v.is_finite().then_some(v)
}

#[cfg(windows)]
fn path() -> Option<String> {
    Settings::dir().map(|d| format!("{d}\\settings.ini"))
}

impl Settings {
    /// `%APPDATA%\Goby`, where the settings live.
    #[cfg(windows)]
    pub fn dir() -> Option<String> {
        std::env::var("APPDATA").ok().map(|d| format!("{}\\Goby", d.trim_end_matches('\\')))
    }

    pub fn parse(text: &str) -> Settings {
        // Notepad can put a byte-order mark in front of the first key when it saves.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut s = Settings::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            let v = v.trim();
            match k.trim() {
                "input" => s.input = v.to_string(),
                "output" => s.output = v.to_string(),
                "voice" => s.voice = v.to_string(),
                "semitones" => s.semitones = number(v, s.semitones, -12.0, 12.0),
                "volume" => s.volume = number(v, s.volume, 0.0, 2.0),
                "monitor" => s.monitor = v == "1",
                "enabled" => s.enabled = v != "0",
                "mic_gain_db" => s.mic_gain_db = number(v, s.mic_gain_db, -12.0, 24.0),
                "gate" => s.gate = v == "1",
                "gate_db" => s.gate_db = number(v, s.gate_db, -80.0, -10.0),
                "exclusive_mic" => s.exclusive = v == "1",
                "routed" => s.routed = list(v),
                "unroute" => s.unroute = list(v),
                "cable_named_tried" => s.cable_named_tried = v == "1",
                "tray" => s.tray = v != "0",
                "tray_told" => s.tray_told = v == "1",
                "hotkeys" => s.hotkeys = v != "0",
                "favorites" => s.favorites = list(v),
                "presets" => s.presets = presets(v),
                "updates" => s.updates = v != "0",
                "update_checked" => s.update_checked = whole(v).unwrap_or(0),
                "dry_mix" => s.dry_mix = number(v, s.dry_mix, 0.0, 1.0),
                "lang" if crate::lang::find(v).is_some() => s.lang = v.to_string(),
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
            "input={}\noutput={}\nvoice={}\nsemitones={}\nvolume={}\nmonitor={}\nenabled={}\nmic_gain_db={}\ngate={}\ngate_db={}\ndry_mix={}\nexclusive_mic={}\nrouted={}\nunroute={}\ncable_named_tried={}\ntray={}\ntray_told={}\nhotkeys={}\nfavorites={}\npresets={}\nupdates={}\nupdate_checked={}\nlang={}\n",
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
            self.lang,
        )
    }

    #[cfg(windows)]
    pub fn load() -> Settings {
        let text = path().and_then(|p| files::read(&p)).and_then(|b| String::from_utf8(b).ok());
        text.map(|t| Settings::parse(&t)).unwrap_or_default()
    }

    #[cfg(windows)]
    pub fn save(&self) {
        if let Some(dir) = Settings::dir() {
            let _ = files::create_dirs(&dir);
            // Write then rename, so a crash mid-write never leaves a half file behind.
            let p = format!("{dir}\\settings.ini");
            let tmp = format!("{p}.tmp");
            if files::write(&tmp, self.render().as_bytes()) {
                let _ = files::rename(&tmp, &p);
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
            exclusive: true,
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
            lang: "ru".into(),
        };
        assert_eq!(Settings::parse(&s.render()), s);
    }

    #[test]
    fn old_files_give_the_mic_back() {
        // Up to 0.3.0 exclusive mode was on by default, which kept every other program
        // off the microphone; a file saved then must not carry that on.
        assert!(!Settings::parse("exclusive=1\n").exclusive);
        assert!(!Settings::default().exclusive);
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
    fn numbers_that_are_not_finite_fall_back_to_defaults() {
        let s =
            Settings::parse("voice=demon\nsemitones=NaN\nvolume=nan\nmic_gain_db=-NaN\ndry_mix=inf\ngate_db=-inf\n");
        let d = Settings::default();
        assert_eq!(
            (s.semitones, s.volume, s.mic_gain_db, s.dry_mix, s.gate_db),
            (d.semitones, d.volume, d.mic_gain_db, d.dry_mix, d.gate_db)
        );
        assert_eq!(Settings::parse("volume=1e9\n").volume, 2.0, "finite but wild values are still clamped");
    }

    #[test]
    fn a_byte_order_mark_does_not_cost_the_first_line() {
        let s = Settings::parse("\u{feff}input={0.0.1.00000000}.{abc}\nvoice=demon\nsemitones=3\n");
        assert_eq!(s.input, "{0.0.1.00000000}.{abc}");
        assert_eq!((s.voice.as_str(), s.semitones), ("demon", 3.0));
        let s = Settings::parse("\u{feff}voice=demon\nsemitones=3\n");
        assert_eq!(s.semitones, 3.0, "a marked voice= line still says the file is a new one");
    }

    #[test]
    fn a_language_goby_does_not_have_is_auto() {
        assert_eq!(Settings::parse("lang=en\n").lang, "en");
        assert_eq!(Settings::parse("lang=xx\n").lang, "auto");
        assert_eq!(Settings::parse("lang=auto\n").lang, "auto");
        assert_eq!(Settings::parse("").lang, "auto");
    }

    fn std_decimal(s: &str) -> Option<u32> {
        s.parse::<f32>().ok().filter(|x| x.is_finite()).map(f32::to_bits)
    }

    #[test]
    fn decimal_reads_what_std_reads() {
        let odd = [
            "",
            "+",
            "-",
            ".",
            "-.",
            "+.",
            "e",
            "e5",
            ".e5",
            "1e",
            "1e+",
            "1e-",
            "1e+-5",
            "1ee5",
            "1e5.0",
            "1.2.3",
            " 1",
            "1 ",
            "1_0",
            "0x10",
            "١",
            "+-1",
            "--1",
            "++1",
            "nan",
            "NaN",
            "-NaN",
            "inf",
            "-inf",
            "+infinity",
            "Infinity",
            "0",
            "-0",
            "+0",
            "00",
            "0.",
            ".0",
            "-.5",
            "5.",
            "1.e5",
            "1E5",
            "1e+5",
            "1e-5",
            "1e0",
            "0e999",
            "0e-999",
            "1e38",
            "3.4028235e38",
            "3.4028236e38",
            "3.5e38",
            "1e39",
            "1e400",
            "-1e400",
            "1e-45",
            "1e-46",
            "1e-400",
            "1e99999999999",
            "1e-99999999999",
            "0.000000000000000000000000000001e30",
            "123456789012345678901234567890",
            "1.23456789012345678901234567890",
            "0.30000001192092896",
            "16777217",
            "16777216.5",
            "33554433",
            "0.1",
            "0.2",
            "0.3",
            "1.1",
            "-12.000001",
            "9",
            "-80",
            "24",
        ];
        for s in odd {
            assert_eq!(decimal(s).map(f32::to_bits), std_decimal(s), "{s:?}");
        }
    }

    #[test]
    fn decimal_reads_back_every_value_settings_can_hold() {
        // Every f32 in the widest knob range, gate_db's -80 up to mic_gain_db's 24, as
        // `render` writes them and in scientific form, read back to the same bits. A
        // stride keeps it quick while still crossing every exponent many times over.
        let mut checked = 0;
        for lo in [0.0f32, -0.0] {
            let mut x = lo;
            while x.abs() <= 80.0 {
                for s in [format!("{x}"), format!("{x:e}"), format!("{x:.3}")] {
                    assert_eq!(decimal(&s).map(f32::to_bits), std_decimal(&s), "{s}");
                }
                checked += 1;
                x = f32::from_bits(x.to_bits() + 4099);
            }
        }
        assert!(checked > 500_000);
        // And short hand-typed decimals, which need not be any float's shortest form.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..200_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let digits = (seed % 1_000_000_000_000) as i64 - 500_000_000_000;
            let point = (seed >> 40) as usize % 13;
            let mut s = digits.abs().to_string();
            while s.len() <= point {
                s.insert(0, '0');
            }
            s.insert(s.len() - point, '.');
            if digits < 0 {
                s.insert(0, '-');
            }
            assert_eq!(decimal(&s).map(f32::to_bits), std_decimal(&s), "{s}");
        }
    }

    #[test]
    fn whole_reads_what_std_reads() {
        for s in [
            "",
            "+",
            "-",
            "-0",
            "0",
            "+0",
            "007",
            "+7",
            "++7",
            " 7",
            "7 ",
            "1.0",
            "1e3",
            "١",
            "18446744073709551615",
            "18446744073709551616",
            "99999999999999999999",
            "4294967295",
            "4294967296",
            "1794000000",
        ] {
            assert_eq!(whole(s), s.parse::<u64>().ok(), "{s:?}");
            assert_eq!(whole(s).and_then(|n| u32::try_from(n).ok()), s.parse::<u32>().ok(), "{s:?}");
        }
    }

    #[test]
    fn windows_line_endings_empty_values_and_huge_numbers() {
        let s = Settings::parse("voice=demon\r\nsemitones=3.5\r\nvolume=\r\nupdate_checked=17\r\nlang=ru\r\n");
        assert_eq!((s.voice.as_str(), s.semitones, s.volume), ("demon", 3.5, 1.0));
        assert_eq!((s.update_checked, s.lang.as_str()), (17, "ru"));
        let s = Settings::parse("voice=demon\nsemitones=\ngate_db=\nupdate_checked=\nmic_gain_db=-\ndry_mix=.\n");
        let d = Settings::default();
        assert_eq!((s.semitones, s.gate_db, s.update_checked), (d.semitones, d.gate_db, 0));
        assert_eq!((s.mic_gain_db, s.dry_mix), (d.mic_gain_db, d.dry_mix));
        let s = Settings::parse(
            "voice=demon\nsemitones=-1e30\nvolume=1e39\nmic_gain_db=99999999999999999999999\nupdate_checked=18446744073709551616\n",
        );
        assert_eq!((s.semitones, s.volume, s.mic_gain_db, s.update_checked), (-12.0, 1.0, 24.0, 0));
        assert_eq!(Settings::parse("update_checked=+18446744073709551615\n").update_checked, u64::MAX);
    }

    #[test]
    fn junk_falls_back_to_defaults() {
        let s = Settings::parse("semitones=banana\nvolume=99\nnoise");
        assert_eq!(s.semitones, Settings::default().semitones);
        assert_eq!(s.volume, 2.0);
    }
}
