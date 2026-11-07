//! The text Goby shows, from one file per language in `lang/`, built into the executable.
//! Each line of a file is `key = text`; `#` starts a comment, `\n` a new line, and `{name}`
//! marks a value the app fills in. A key a file leaves out is shown in English.

use std::sync::OnceLock;

/// A file from `lang/`, packed while compiling: comments and spaces around keys go, `\n`
/// becomes a new line, and what is left is `key\0text\0` for every line.
macro_rules! file {
    ($code:literal) => {{
        const TEXT: &str = include_str!(concat!("../lang/", $code, ".lang"));
        const PACKED: [u8; pack(TEXT.as_bytes(), &mut [])] = {
            let mut out = [0; pack(TEXT.as_bytes(), &mut [])];
            pack(TEXT.as_bytes(), &mut out);
            out
        };
        match std::str::from_utf8(&PACKED) {
            Ok(packed) => packed,
            Err(_) => panic!(concat!("lang/", $code, ".lang is not UTF-8")),
        }
    }};
}

/// Every language: its code, its name in itself, and its file. English comes first, as
/// the one the others fall back to. A new language is one more file and one more line.
pub const LANGUAGES: &[(&str, &str, &str)] = &[("en", "English", file!("en")), ("ru", "Русский", file!("ru"))];

/// Writes `text` packed into `out` as far as it fits, and says how long it is in full.
const fn pack(text: &[u8], out: &mut [u8]) -> usize {
    const fn trim(t: &[u8], mut from: usize, mut to: usize) -> (usize, usize) {
        while from < to && t[from].is_ascii_whitespace() {
            from += 1;
        }
        while to > from && t[to - 1].is_ascii_whitespace() {
            to -= 1;
        }
        (from, to)
    }
    const fn put(out: &mut [u8], n: usize, b: u8) -> usize {
        if n < out.len() {
            out[n] = b;
        }
        n + 1
    }
    let (mut n, mut line) = (0, 0);
    while line < text.len() {
        let mut end = line;
        while end < text.len() && text[end] != b'\n' {
            end += 1;
        }
        let (from, to) = trim(text, line, end);
        let mut eq = from;
        while eq < to && text[eq] != b'=' {
            eq += 1;
        }
        if eq < to && text[from] != b'#' {
            let (mut i, key_end) = trim(text, from, eq);
            while i < key_end {
                n = put(out, n, text[i]);
                i += 1;
            }
            n = put(out, n, 0);
            let (mut i, to) = trim(text, eq + 1, to);
            while i < to {
                let new_line = text[i] == b'\\' && i + 1 < to && text[i + 1] == b'n';
                n = put(out, n, if new_line { b'\n' } else { text[i] });
                i += 1 + new_line as usize;
            }
            n = put(out, n, 0);
        }
        line = end + 1;
    }
    n
}

/// One language's lines, in the order of its file.
type Table = Vec<(&'static str, &'static str)>;

// Inlined, it is copied once per language: 1.4 KB more in the executable.
#[inline(never)]
fn parse(packed: &'static str) -> Table {
    let (mut table, mut parts) = (Table::new(), packed.split('\0'));
    while let (Some(key), Some(text)) = (parts.next(), parts.next()) {
        table.push((key, text));
    }
    table
}

fn tables() -> &'static [Table] {
    static TABLES: OnceLock<Vec<Table>> = OnceLock::new();
    TABLES.get_or_init(|| LANGUAGES.iter().map(|(_, _, packed)| parse(packed)).collect())
}

// The app has one language at a time. Tests run side by side, each in the language it
// picked, so there it is kept per thread.
#[cfg(not(test))]
static CURRENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The language in use, as its place in `LANGUAGES`.
#[cfg(not(test))]
pub fn now() -> usize {
    CURRENT.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(test)]
pub fn now() -> usize {
    CURRENT.get()
}

/// Switches to `choice` from settings.ini: a language code, or "auto" for `system`, the
/// display language as a tag such as "ru-RU". Anything else is "auto" too.
pub fn choose(choice: &str, system: &str) {
    let i = resolve(choice, system);
    #[cfg(not(test))]
    CURRENT.store(i, std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    CURRENT.set(i);
}

/// Where the language `choose` would pick is in `LANGUAGES`.
pub fn resolve(choice: &str, system: &str) -> usize {
    let code = if find(choice).is_some() { choice } else { system.split('-').next().unwrap_or("") };
    find(code).unwrap_or(0)
}

/// Where the language with this code is in `LANGUAGES`.
pub fn find(code: &str) -> Option<usize> {
    LANGUAGES.iter().position(|(c, _, _)| *c == code)
}

/// The text for `key` in the current language, else in English, else the key itself.
pub fn tr(key: &'static str) -> &'static str {
    look(tables(), now(), |k| k == key).unwrap_or(key)
}

/// A voice's name, under `voice.` and its id; the id itself if no file has one.
pub fn voice(id: &'static str) -> &'static str {
    look(tables(), now(), |k| k.strip_prefix("voice.") == Some(id)).unwrap_or(id)
}

/// The text in language `at`, else in English, under the first key `is` accepts. A look
/// through a couple of hundred short keys takes well under a microsecond.
fn look(tables: &'static [Table], at: usize, is: impl Fn(&str) -> bool) -> Option<&'static str> {
    let get = |t: &'static Table| t.iter().find(|(k, _)| is(k)).map(|(_, text)| *text);
    get(&tables[at]).or_else(|| get(&tables[0]))
}

/// `tr`, with each `{name}` replaced by its value.
pub fn trf(key: &'static str, values: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = tr(key);
    while let Some((before, after)) = rest.split_once('{') {
        let (name, after) = after.split_once('}').unwrap_or((after, ""));
        out.push_str(before);
        out.push_str(values.iter().find(|(n, _)| *n == name).map_or(name, |(_, v)| v));
        rest = after;
    }
    out + rest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text.split('{').skip(1).filter_map(|s| s.split_once('}')).map(|(n, _)| n).collect();
        found.sort();
        found
    }

    fn packed(text: &str) -> String {
        let mut out = vec![0; pack(text.as_bytes(), &mut [])];
        pack(text.as_bytes(), &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn files_are_packed_into_keys_and_texts() {
        let text = "# A comment = not a line\n  a = x\\ny \r\nb=2 = 3\n\nno equals sign\nc = a \\ b\n";
        assert_eq!(packed(text), "a\0x\ny\0b\x002 = 3\0c\0a \\ b\0");
        let table = parse(packed(text).leak());
        assert_eq!(table, [("a", "x\ny"), ("b", "2 = 3"), ("c", "a \\ b")]);
    }

    #[test]
    fn every_file_has_the_same_keys_once_and_the_same_placeholders() {
        let tables = tables();
        let english = &tables[0];
        for ((code, name, _), table) in LANGUAGES.iter().zip(tables) {
            for (i, (key, text)) in table.iter().enumerate() {
                assert!(!table[..i].iter().any(|(k, _)| k == key), "{code}: {key} is there twice");
                let Some((_, en)) = english.iter().find(|(k, _)| k == key) else {
                    panic!("{code}: {key} is not in en.lang");
                };
                assert_eq!(placeholders(text), placeholders(en), "{code}: {key}");
                assert!(!text.is_empty(), "{code}: {key} is empty");
            }
            for (key, _) in english {
                assert!(table.iter().any(|(k, _)| k == key), "{code}: {key} is missing");
            }
            assert!(!name.is_empty() && code.len() == 2, "{code}");
            // Every line that is not a comment is a key and its text.
            let path = format!("{}/lang/{code}.lang", env!("CARGO_MANIFEST_DIR"));
            let raw = std::fs::read_to_string(path).unwrap();
            let lines = raw.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
            assert_eq!(
                lines.clone().filter(|l| l.contains(" = ")).count(),
                table.len(),
                "{code}: a line is not key = text"
            );
            assert!(lines.clone().all(|l| l.contains(" = ")), "{code}: a line is not key = text");
        }
    }

    /// Every `tr("…")` and `trf("…"` in the source, and the voices' keys.
    fn used() -> Vec<String> {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    // Tests may look up keys that are not there on purpose.
                    let text = text.split("mod tests {").next().unwrap();
                    for call in ["tr(\"", "trf(\""] {
                        for (at, _) in text.match_indices(call) {
                            let before = text[..at].chars().next_back().unwrap_or(' ');
                            if before.is_alphanumeric() || before == '_' {
                                continue;
                            }
                            let rest = &text[at + call.len()..];
                            out.push(rest[..rest.find('"').unwrap()].to_string());
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out.extend(crate::voices::ALL.iter().map(|v| format!("voice.{}", v.id)));
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn every_key_the_code_uses_is_there_and_every_key_there_is_used() {
        let used = used();
        let english: Vec<&str> = tables()[0].iter().map(|(k, _)| *k).collect();
        assert!(used.len() > 100, "the scan found {} keys", used.len());
        for key in &used {
            assert!(english.contains(&key.as_str()), "{key} is used but not in en.lang");
        }
        for key in &english {
            assert!(used.iter().any(|k| k == key), "{key} is in en.lang but never used");
        }
    }

    #[test]
    fn text_is_looked_up_with_english_behind_it() {
        choose("ru", "en-US");
        assert_eq!(tr("page.voices"), "Голоса");
        assert_eq!(voice("minion"), "Миньон");
        assert_eq!(voice("no_such_voice"), "no_such_voice");
        assert_eq!(tr("no.such.key"), "no.such.key");
        assert_eq!(trf("tray.effect", &[("voice", "Демон")]), "Эффект: Демон");
        assert!(tr("setup.run").contains('\n'), "\\n is a new line");
        choose("en", "ru-RU");
        assert_eq!(tr("page.voices"), "Voices");
        choose("auto", "ru-RU");
        assert_eq!(now(), 1);
        choose("auto", "de-DE");
        assert_eq!(now(), 0, "a language Goby lacks is English");
        choose("xx", "ru");
        assert_eq!(now(), 1, "an unknown choice follows the system");
    }

    #[test]
    fn a_missing_line_falls_back_to_english() {
        let tables = Box::leak(Box::new([parse("a\0A\0b\0B\0"), parse("b\0Б\0")]));
        let at = |key: &str| look(tables, 1, |k| k == key);
        assert_eq!(at("a"), Some("A"));
        assert_eq!(at("b"), Some("Б"));
        assert_eq!(at("c"), None);
    }
}
