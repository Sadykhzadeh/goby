//! The report Settings saves for a bug report: what a person helping would ask first, as
//! plain text, with nothing in it that names the user or the PC.

use crate::settings::write_decimal;

/// What the report says, as the window knows it. Devices come as one line each: the
/// name Windows gives them and the format they run at, never their ids.
pub struct Facts<'a> {
    pub version: &'a str,
    pub commit: &'a str,
    pub windows: &'a str,
    pub mic: &'a str,
    pub output: &'a str,
    pub cable: bool,
    /// Low latency was asked for, and whether the mic really runs that way.
    pub exclusive: (bool, bool),
    pub latency_ms: f32,
    pub voice: &'a str,
    pub effect: bool,
    pub programs: &'a [String],
}

/// The names to take out wherever they stand.
pub struct Who<'a> {
    pub user: &'a str,
    pub machine: &'a str,
}

pub fn build(f: &Facts, who: &Who) -> String {
    let yes = |on: bool| if on { "found" } else { "not found" };
    let mode = match f.exclusive {
        (false, _) => "shared",
        (true, true) => "exclusive",
        (true, false) => "exclusive was asked, the mic refused it: shared",
    };
    let programs = if f.programs.is_empty() { "none".to_string() } else { f.programs.join(", ") };
    let commit = if f.commit.is_empty() { String::new() } else { format!(" ({})", f.commit) };
    let text = format!(
        "Goby {}{commit}\nWindows {}\nMicrophone: {}\nOutput: {}\nCable: {}\nLatency mode: {mode}\nLatency: {} ms\nVoice: {} (effect {})\nPrograms: {programs}\n",
        f.version,
        f.windows,
        f.mic,
        f.output,
        yes(f.cable),
        write_decimal(f.latency_ms.round()),
        f.voice,
        if f.effect { "on" } else { "off" },
    );
    redact(&text, who)
}

/// Whether `c` could be part of a word, for telling a user name from the same letters
/// inside a longer one.
fn word(c: Option<&u8>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// `text` without the user's folder, the user's and the PC's names, or ids in braces.
/// A path under `X:\Users\` becomes `%USERPROFILE%` whoever the user is, as a device or a
/// program can carry one in its name; the two names are taken out in any case, but only as
/// whole words, so a short one does not eat the letters of other words.
fn redact(text: &str, who: &Who) -> String {
    let lower = text.to_ascii_lowercase();
    let (user, machine) = (who.user.to_ascii_lowercase(), who.machine.to_ascii_lowercase());
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let named = |name: &str| {
            !name.is_empty()
                && rest.starts_with(name)
                && !word(i.checked_sub(1).and_then(|p| bytes.get(p)))
                && !word(bytes.get(i + name.len()))
        };
        if i > 0
            && bytes[i - 1].is_ascii_alphabetic()
            && (rest.starts_with(":\\users\\") || rest.starts_with(":/users/"))
        {
            // Past the name: the user's own when it is that, else up to the next separator.
            let from = i + ":\\users\\".len();
            let end = if lower[from..].starts_with(&user) && !user.is_empty() {
                from + user.len()
            } else {
                from + lower[from..].find(['\\', '/', '\r', '\n', '"']).unwrap_or(lower.len() - from)
            };
            out.pop();
            out.push_str("%USERPROFILE%");
            i = end;
        } else if named(&user) {
            out.push_str("<user>");
            i += user.len();
        } else if named(&machine) {
            out.push_str("<pc>");
            i += machine.len();
        } else if let Some(n) =
            rest.strip_prefix('{').and_then(|r| r.find('}')).filter(|n| *n < 80 && !rest[..*n].contains(' '))
        {
            out.push_str("{id}");
            i += n + 2;
        } else {
            let c = text[i..].chars().next().unwrap_or_default();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(programs: &'a [String], mic: &'a str, output: &'a str) -> Facts<'a> {
        Facts {
            version: "0.5.0",
            commit: "1b5680b",
            windows: "10.0.26200.1234",
            mic,
            output,
            cable: true,
            exclusive: (true, false),
            latency_ms: 27.6,
            voice: "demon",
            effect: true,
            programs,
        }
    }

    #[test]
    fn the_report_says_what_support_asks_first() {
        let programs = ["cs2.exe".to_string(), "discord.exe".to_string()];
        let who = Who { user: "Azer", machine: "DESKTOP-7QK2" };
        let text = build(
            &facts(&programs, "Microphone (USB Audio), 48000 Hz, 2 ch, 16 bit", "CABLE Input, 48000 Hz, 2 ch, 32 bit"),
            &who,
        );
        assert_eq!(
            text,
            "Goby 0.5.0 (1b5680b)\nWindows 10.0.26200.1234\nMicrophone: Microphone (USB Audio), 48000 Hz, 2 ch, 16 bit\n\
             Output: CABLE Input, 48000 Hz, 2 ch, 32 bit\nCable: found\n\
             Latency mode: exclusive was asked, the mic refused it: shared\nLatency: 28 ms\n\
             Voice: demon (effect on)\nPrograms: cs2.exe, discord.exe\n"
        );
        let none = build(
            &Facts { commit: "", cable: false, effect: false, exclusive: (false, false), ..facts(&[], "-", "-") },
            &who,
        );
        assert!(none.starts_with("Goby 0.5.0\n") && none.contains("Cable: not found\nLatency mode: shared\n"));
        assert!(none.ends_with("Voice: demon (effect off)\nPrograms: none\n"));
    }

    #[test]
    fn a_path_under_a_users_folder_never_comes_out() {
        let who = Who { user: "Azer", machine: "DESKTOP-7QK2" };
        // A name that carries the home folder, one that is somebody else's, a program run
        // from a folder with spaces, forward slashes and a different drive.
        let programs = [
            r"C:\Users\Azer\Desktop\Game\game.exe".to_string(),
            r"d:\USERS\Some One\AppData\Local\x.exe".to_string(),
            "e:/users/azer/x.exe e:\\Users\\Azer".to_string(),
            r"C:\Users\Azer".to_string(),
        ];
        let text = build(
            &facts(
                &programs,
                r#"Mic (C:\Users\Azer\AppData\Roaming\Mic.dll) {0.0.1.00000000}.{5a1c-77}"#,
                "Azer's headphones on DESKTOP-7QK2 (desktop-7qk2)",
            ),
            &who,
        );
        for secret in ["azer", "some one", "7qk2", "{0.0", "5a1c"] {
            assert!(!text.to_ascii_lowercase().contains(secret), "{secret}: {text}");
        }
        assert!(!text.contains("\\Users\\") && !text.to_ascii_lowercase().contains("users\\"), "{text}");
        assert!(text.contains("%USERPROFILE%\\Desktop\\Game\\game.exe"), "{text}");
        assert!(text.contains("<user>'s headphones on <pc> (<pc>)"), "{text}");
        assert!(text.contains("Mic (%USERPROFILE%\\AppData\\Roaming\\Mic.dll) {id}.{id}"), "{text}");
        // What is not private is left alone.
        assert!(text.contains("Windows 10.0.26200.1234") && text.contains("Cable: found"));
    }

    #[test]
    fn a_name_is_taken_out_only_as_a_word() {
        let who = Who { user: "Al", machine: "" };
        assert_eq!(redact("Al, Alex, Cal and al_x, (AL)", &who), "<user>, Alex, Cal and al_x, (<user>)");
        let nobody = Who { user: "", machine: "" };
        assert_eq!(redact("Mic {x} {", &nobody), "Mic {id} {");
        // Text that is not ASCII goes through whole, around a name that is not either.
        let who = Who { user: "Азер", machine: "ПК" };
        assert_eq!(redact("Микрофон Азер на ПК — «Азеров»", &who), "Микрофон <user> на <pc> — «<user>ов»");
        assert_eq!(redact("C:\\Users\\", &nobody), "%USERPROFILE%");
    }
}
