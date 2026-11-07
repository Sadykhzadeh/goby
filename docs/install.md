# Installing, updating and building Goby

## Installing and uninstalling

Goby installs per user into `%LOCALAPPDATA%\Programs\Goby`, with a Start menu shortcut
and an entry under **Settings > Apps**. Running a newer download updates it in place.
Uninstall it from Settings > Apps: it closes Goby, hands the programs you chose back to
the default microphone, gives the cable its VB-Audio names back, and removes its
shortcuts, settings and folder.

For scripts and package managers:

```
goby-0.4.0-windows-x64.exe --install [--desktop]   # silent install, optional desktop shortcut
goby.exe --uninstall --quiet                        # silent uninstall
goby-0.4.0-windows-x64.exe --portable               # run without installing or asking
```

## Updates

Once a day, a little after it starts, Goby asks GitHub and a mirror on the author's own server for the manifest of
their latest release: a few lines naming the version, the size and SHA-256 of the
executable, and where to download it. Every release is published to both, and the
newest version either one offers wins, so Goby keeps updating while one of them is down.
When a newer one is out, Settings says so and installs it with one click; a portable copy
gets a link to the release page instead.

Neither host is trusted with what Goby runs. The manifest counts only with a valid
ECDSA P-256 signature from the release key, whose public half is built into Goby and whose
private half never leaves the author. The download must match the size and SHA-256 the
signed manifest names, it is checked again after it is written, and it stays locked
against writes until it starts. A manifest for an older version than the running one is
ignored, so an old release cannot be replayed. Signatures and hashes go through Windows'
own CNG and the downloads through WinHTTP over HTTPS: no extra code to trust and almost
nothing added to the executable. Nothing else is sent, and **Check for updates** under
Settings turns it off.

## Building

Needs Windows 10 or 11 and Rust 1.88 or newer. Mica needs Windows 11 22H2; older systems
get a solid background.

```
cargo build --release
cargo test --release
```

The binary lands in `target/release/goby.exe`. Settings live in `%APPDATA%\Goby\settings.ini`.
