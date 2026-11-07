//! The text Goby shows, from one file per language in `lang/`, built into the executable.
//! Each line of a file is `key = text`; `#` starts a comment, `\n` a new line, and `{name}`
//! marks a value the app fills in. A key a file leaves out is shown in English.

use std::sync::OnceLock;

/// A file from `lang/`, packed while compiling: its texts only, `\n` turned into a new line,
/// as `text\0` in the order of en.lang's keys, empty where the file lacks a line, and the
/// Russian alphabet a byte a letter (see `texts`). The keys themselves are not kept: `tr!`
/// turns each into its place in en.lang while compiling.
macro_rules! file {
    ($code:literal) => {{
        const TEXT: &str = include_str!(concat!("../lang/", $code, ".lang"));
        const PACKED: [u8; texts(TEXT.as_bytes(), EN, &mut [])] = {
            let mut out = [0; texts(TEXT.as_bytes(), EN, &mut [])];
            texts(TEXT.as_bytes(), EN, &mut out);
            out
        };
        &PACKED
    }};
}

/// Every language: its code, its name in itself, and its file. English comes first, as
/// the one the others fall back to. A new language is one more file and one more line.
pub const LANGUAGES: &[(&str, &str, &[u8])] = &[("en", "English", file!("en")), ("ru", "Русский", file!("ru"))];

const EN: &[u8] = include_str!("../lang/en.lang").as_bytes();

const fn trim(t: &[u8], mut from: usize, mut to: usize) -> (usize, usize) {
    while from < to && t[from].is_ascii_whitespace() {
        from += 1;
    }
    while to > from && t[to - 1].is_ascii_whitespace() {
        to -= 1;
    }
    (from, to)
}

/// The first `key = text` line at or after `at`: where its key and its text start and end,
/// trimmed, and where the next line starts. Comments, blank lines and lines without `=` are
/// passed over.
const fn line(t: &[u8], mut at: usize) -> Option<[usize; 5]> {
    while at < t.len() {
        let mut end = at;
        while end < t.len() && t[end] != b'\n' {
            end += 1;
        }
        let (from, to) = trim(t, at, end);
        let mut eq = from;
        while eq < to && t[eq] != b'=' {
            eq += 1;
        }
        if eq < to && t[from] != b'#' {
            let ((k0, k1), (v0, v1)) = (trim(t, from, eq), trim(t, eq + 1, to));
            return Some([k0, k1, v0, v1, end + 1]);
        }
        at = end + 1;
    }
    None
}

const fn put(out: &mut [u8], n: usize, b: u8) -> usize {
    if n < out.len() {
        out[n] = b;
    }
    n + 1
}

/// Where `key` is among en.lang's keys. Used while compiling, where a key en.lang lacks
/// stops the build.
pub const fn index(key: &str) -> usize {
    let (key, mut i, mut at) = (key.as_bytes(), 0, 0);
    while let Some([k0, k1, _, _, next]) = line(EN, at) {
        let mut same = k1 - k0 == key.len();
        let mut j = 0;
        while same && j < key.len() {
            same = EN[k0 + j] == key[j];
            j += 1;
        }
        if same {
            return i;
        }
        (i, at) = (i + 1, next);
    }
    panic!("a key that is not in en.lang")
}

/// The line of `file` whose key is `en[k0..k1]`, looked for from `from` to `to`.
const fn seek(file: &[u8], mut from: usize, to: usize, en: &[u8], k0: usize, k1: usize) -> Option<[usize; 5]> {
    while let Some(l) = line(file, from) {
        if l[0] >= to {
            break;
        }
        if l[1] - l[0] == k1 - k0 {
            let mut i = 0;
            while i < k1 - k0 && file[l[0] + i] == en[k0 + i] {
                i += 1;
            }
            if i == k1 - k0 {
                return Some(l);
            }
        }
        from = l[4];
    }
    None
}

/// Writes the texts of `file` packed into `out` as far as they fit, one for each key of
/// `en` in its order, and says how long they are in full. А to я, two bytes in UTF-8, go
/// in one, 0x80 to 0xBF: bytes that never start a character in UTF-8, so `parse` can tell
/// them apart. That makes Russian text 40 % smaller.
const fn texts(file: &[u8], en: &[u8], out: &mut [u8]) -> usize {
    let (mut n, mut at, mut last) = (0, 0, 0);
    while let Some([k0, k1, _, _, next]) = line(en, at) {
        // Files keep en.lang's order, so the search goes on from the line found last and
        // only wraps around for a file that does not.
        let found = match seek(file, last, file.len(), en, k0, k1) {
            Some(l) => Some(l),
            None => seek(file, 0, last, en, k0, k1),
        };
        if let Some([_, _, mut i, to, after]) = found {
            while i < to {
                let next = if i + 1 < to { file[i + 1] } else { 0 };
                let (b, len) = match (file[i], next) {
                    (b'\\', b'n') => (b'\n', 2),
                    (0xD0, 0x90..=0xBF) => (next - 0x10, 2), // А to п
                    (0xD1, 0x80..=0x8F) => (next + 0x30, 2), // р to я
                    (b, _) => (b, 1),
                };
                n = put(out, n, b);
                i += len;
            }
            last = after;
        }
        n = put(out, n, 0);
        at = next;
    }
    n
}

/// One language's texts, in the order of en.lang's keys; empty where it falls back to English.
type Table = Vec<&'static str>;

// Inlined, it is copied once per language.
#[inline(never)]
fn parse(packed: &[u8]) -> Table {
    let mut utf8 = Vec::with_capacity(packed.len() * 2);
    let mut continuing = 0;
    for &b in packed {
        if continuing == 0 && (0x80..0xC0).contains(&b) {
            utf8.extend(if b < 0xB0 { [0xD0, b + 0x10] } else { [0xD1, b - 0x30] });
        } else {
            continuing =
                if continuing > 0 { continuing - 1 } else { (b >= 0xC0) as u8 + (b >= 0xE0) as u8 + (b >= 0xF0) as u8 };
            utf8.push(b);
        }
    }
    let mut table = Table::new();
    for text in String::from_utf8(utf8).unwrap_or_default().leak().split('\0') {
        table.push(text);
    }
    table.pop(); // after the last \0
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

/// The text for a key of en.lang in the current language, else in English. The key becomes
/// a number while compiling, so no key is kept in the executable, and one en.lang lacks
/// stops the build.
macro_rules! tr {
    ($key:literal) => {
        $crate::lang::text(const { $crate::lang::index($key) })
    };
}
pub(crate) use tr;

/// `tr!`, with each `{name}` replaced by its value.
macro_rules! trf {
    ($key:literal, $values:expr) => {
        $crate::lang::fill($crate::lang::tr!($key), $values)
    };
}
pub(crate) use trf;

/// Text `i`, in the order of en.lang, in the current language, else in English.
// Inlined, it is copied into each of a hundred callers.
#[inline(never)]
pub fn text(i: usize) -> &'static str {
    look(tables(), now(), i)
}

/// Text `i` in language `at`, else in English.
fn look(tables: &'static [Table], at: usize, i: usize) -> &'static str {
    Some(tables[at][i]).filter(|t| !t.is_empty()).unwrap_or(tables[0][i])
}

/// `text` with each `{name}` replaced by its value.
pub fn fill(text: &'static str, values: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = text;
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

    fn packed(text: &str, en: &str) -> Vec<u8> {
        let mut out = vec![0; texts(text.as_bytes(), en.as_bytes(), &mut [])];
        texts(text.as_bytes(), en.as_bytes(), &mut out);
        out
    }

    /// Every key and its text in a file, in its order.
    fn lines(text: &str) -> Vec<(&str, String)> {
        let (mut out, mut at) = (Vec::new(), 0);
        while let Some([k0, k1, v0, v1, next]) = line(text.as_bytes(), at) {
            out.push((&text[k0..k1], text[v0..v1].replace("\\n", "\n")));
            at = next;
        }
        out
    }

    #[test]
    fn files_are_packed_into_texts_in_the_order_of_english() {
        let en = "# A comment = not a line\n  a = x\\ny \r\nb=2 = 3\n\nno equals sign\nc = a \\ b\n";
        assert_eq!(packed(en, en), b"x\ny\x002 = 3\0a \\ b\0");
        assert_eq!(parse(&packed(en, en)), ["x\ny", "2 = 3", "a \\ b"]);
        // The Russian alphabet takes a byte a letter; the rest of Unicode stays as it was.
        let text = "Ёжик «в тумане» — ЯБЛОКО, ёлка и рыба €𝄞";
        let line = format!("a = {text}");
        assert_eq!(packed(&line, "a = x").len(), text.len() - 24 + 1);
        assert_eq!(parse(&packed(&line, "a = x")), [text]);
        // Out of order, a line missing, and one English lacks.
        assert_eq!(packed("c = C\nd = D\nb = B\n", en), b"\0B\0C\0");
        let keys: Vec<&str> = lines(en).iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, ["a", "b", "c"]);
    }

    #[test]
    fn every_file_has_the_same_keys_once_and_the_same_placeholders() {
        let read = |code: &str| {
            let path = format!("{}/lang/{code}.lang", env!("CARGO_MANIFEST_DIR"));
            std::fs::read_to_string(path).unwrap()
        };
        let en_file = read("en");
        let english = lines(&en_file);
        for (i, (key, _)) in english.iter().enumerate() {
            assert_eq!(index(key), i, "{key}");
        }
        for (at, (code, name, _)) in LANGUAGES.iter().enumerate() {
            let file = read(code);
            let table = lines(&file);
            for (i, (key, text)) in table.iter().enumerate() {
                assert!(!table[..i].iter().any(|(k, _)| k == key), "{code}: {key} is there twice");
                let Some((_, en)) = english.iter().find(|(k, _)| k == key) else {
                    panic!("{code}: {key} is not in en.lang");
                };
                assert_eq!(placeholders(text), placeholders(en), "{code}: {key}");
                assert!(!text.is_empty(), "{code}: {key} is empty");
            }
            for (i, (key, _)) in english.iter().enumerate() {
                let Some((_, text)) = table.iter().find(|(k, _)| k == key) else {
                    panic!("{code}: {key} is missing");
                };
                assert_eq!(look(tables(), at, i), text, "{code}: {key} is packed wrong");
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

    /// Every `tr!("…")` and `trf!("…"` in the source, and the voices' keys.
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
                    for call in ["tr!(\"", "trf!(\""] {
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
        let file = include_str!("../lang/en.lang");
        let english: Vec<&str> = lines(file).into_iter().map(|(k, _)| k).collect();
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
        assert_eq!(tr!("page.voices"), "Голоса");
        assert_eq!(crate::voices::ALL[crate::voices::index_of("minion").unwrap()].name(), "Миньон");
        assert_eq!(trf!("tray.effect", &[("voice", "Демон")]), "Эффект: Демон");
        assert!(tr!("setup.run").contains('\n'), "\\n is a new line");
        choose("en", "ru-RU");
        assert_eq!(tr!("page.voices"), "Voices");
        choose("auto", "ru-RU");
        assert_eq!(now(), 1);
        choose("auto", "de-DE");
        assert_eq!(now(), 0, "a language Goby lacks is English");
        choose("xx", "ru");
        assert_eq!(now(), 1, "an unknown choice follows the system");
    }

    #[test]
    fn a_missing_line_falls_back_to_english() {
        let en = "a = A\nb = B\n";
        let tables = Box::leak(Box::new([parse(&packed(en, en)), parse(&packed("b = Б\n", en))]));
        assert_eq!(look(tables, 1, 0), "A");
        assert_eq!(look(tables, 1, 1), "Б");
    }
}
