# Translating Goby

Goby speaks English and Russian. By default it follows the Windows display language
(on the website, the browser's); **Settings > General > Language** picks one for good,
and the choice is saved in `settings.ini` as `lang=auto`, `lang=en` or `lang=ru`.

Every text Goby shows lives in one file per language under [`lang/`](../lang): the
window, voice and group names, the tray menu, the install and uninstall dialogs, update
notes and error messages. The files are built into `goby.exe`, so it stays one file.

## The format

Plain UTF-8, one text per line:

```
# A comment.
settings.tray.title = Keep running in the tray
setup.run = Just run it\nWithout installing anything
tray.effect = Effect: {voice}
```

- `key = text`. The key stays as it is in every language; only the text after ` = ` is
  translated.
- `\n` starts a new line. The installer's buttons use it to put a note under the label.
- `{name}` is filled in by Goby, here with the voice that is on. Keep every `{…}` of the
  English text, spelled the same; move it wherever the sentence needs it.
- A line that starts with `#` is a comment. Empty lines are ignored.
- A key missing from a file shows in English, so a half-done translation still runs; the
  tests only pass once every key is there.

`en.lang` is the reference: every key the code uses is there.

## Adding a language

1. Copy `lang/en.lang` to `lang/<code>.lang`, with the two-letter code Windows and browsers
   use for it (`de`, `uk`, `tr`, …), and translate the texts.
2. Add one line to `LANGUAGES` in [`src/lang.rs`](../src/lang.rs), with the language's
   name in itself:

   ```rust
   ("de", "Deutsch", file!("de")),
   ```

3. Run `cargo test --release`. It checks that the new file has every key of `en.lang` and
   no other, no key twice, and the same `{…}` placeholders in every text.

Then look at the window at its smallest size: a text that runs longer than the English
one can be cut off with an ellipsis.
