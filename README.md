# Goby

A small, fast voice changer for Windows. It takes your real microphone, turns you into a
minion, a demon, a robot, a walkie-talkie or one of 61 voices, and hands the result to any
game or chat app through a virtual audio cable.

One ~500 KB executable that installs itself for your user, no administrator rights
needed, or runs without installing. No background service, no driver of its own, no
account, nothing sent anywhere. The window is plain Win32 in the Windows 11 style: Mica,
the system accent colour, light or dark to match Windows, English or Russian to match
your display language.

## Voices

61 voices in six groups, each built from a handful of classic effects:

| Group | Voices |
|---|---|
| Basics | Off, Minion, Helium, Giant, Robot, Radio |
| People | Kid, Baby, Woman, Man, Deep voice, Grandpa, Grandma, Teen, Pirate, Nervous, Tipsy, Whisper, Sleepy, Announcer |
| Creatures | Gremlin, Rodent, Ogre, Troll, Demon, Zombie, Ghost, Possessed, Vampire, Dragon, Swarm, Alien |
| Robots | Android, Dictator, Cyborg, Mutant, 8-bit, Synth choir, Hologram, Spaceship |
| Places | Cave, Cathedral, Bathroom, Stadium, Underwater, Canyon, Tin can |
| Devices | Phone, Megaphone, Walkie-talkie, Old radio, Drive-thru, Cassette |
| Fun | Squeaky choir, Opera, Choir, Karaoke, Glitch, Duet, Jet, Squeaky robot |

The effects underneath are a pitch shifter, harmony voices, vibrato and random wobble,
ring modulation, whisper, a 16-band vocoder, comb, chorus and flanger, tremolo, filters,
saturation and hard clipping, a bit crusher, voice-gated static, echo and reverb. Every
voice is a short recipe in [`src/voices.rs`](src/voices.rs); adding one is a single line.

Each gallery tile shows the voice's own waveform: a test tone is run through the recipe
when the app starts and the result is drawn.

## Using it in CS2 (or Discord, OBS, anything)

1. Run the downloaded `goby-0.3.0-windows-x64.exe` and choose **Install Goby** (or
   **Just run it**).
2. Goby needs [VB-Audio Virtual Cable](https://vb-audio.com/Cable/) (free) to reach other
   programs. Without it Goby opens on three steps: download it, install it, and wait
   while Goby looks for it every two seconds. Once the cable shows up Goby sends the voice
   into it, no restart needed, and Windows asks once for permission to rename it, so it
   shows up as **Goby Mic** everywhere instead of "CABLE Output". VB-Cable is VB-Audio's
   own driver and is installed from their site; Goby does not ship it.
3. Open **Programs** on the left. Under **Recently used the microphone** (the same list
   Windows keeps for its privacy page), press **+** next to CS2, Discord or OBS; **Add a
   program** also has **Other program…**, which opens the usual file picker. The page
   shows whether each program hears Goby right now, will once it starts, or will once it
   opens the microphone. Goby pins
   only the chosen programs to Goby Mic, the way the Volume mixer's per-app settings do.
   Everything else keeps your real microphone and nothing system-wide changes. The ×
   next to a program hands it back.

CS2 has no microphone picker of its own, which is exactly why the per-program switch is
there. Goby never offers the cable as its own microphone, so it cannot feed itself back in.

The main window holds what you change while playing: the voice, the effect switch (it
keeps the chosen voice), pitch, volume and *Hear myself* for your headphones. Everything
you set once lives in **Settings**: the microphone and a noise gate that mutes keyboard
clicks and room noise between phrases, the output, how much of your original voice to
mix back in, and the tray, startup and hotkey switches. Gain, the gate threshold and *Low
latency*, which takes the microphone in exclusive mode, fold away under **Advanced**.
Low latency is off by default: while it is on, no other program can record from that
microphone, not even your voice typing.
Arrow keys walk the gallery, Esc leaves Settings and Programs, and a double-click resets
a slider.

Hotkeys work from inside a game: **Ctrl+Alt+G** switches the effect on and off, and
**Ctrl+Alt+Page Down** / **Page Up** step to the next or previous voice (which also
switches the effect back on). Settings shows when another program already holds one of
them, and can turn them off.

Star the voices you use (or right-click them) and they get a **Favorites** tab, which
Goby opens on from then on. The hotkeys then step through the favorites only, and the
tray icon's menu lists them, so a voice can be switched without opening the window.

Tune a voice with pitch, volume or the original-voice mix and **Save as my voice** keeps
it as it is, named after what changed (*Demon +3*, *Robot · 125%*). Saved voices come
first in Favorites, in the tray menu and on the hotkeys; the × on a saved voice deletes it.

Closing the window leaves Goby running in the notification area, so your voice stays on
in the game. Click the icon to bring the window back; right-click it to switch the effect
off or to quit. *Keep running in the tray* under Settings turns this off, and then closing
the window quits. `goby.exe --tray` starts with only the icon, and *Start with Windows*
(off until you turn it on) does that at sign-in. Uninstalling removes it from startup.

## Numbers

Release build on Windows 11 with a USB microphone (fifine) and VB-Cable:

| | |
|---|---|
| Executable | 506 KB, icons, installer, tray and updates included |
| Dependencies | `windows`, `windows-core`, `windows-numerics` (Microsoft's own bindings) |
| CPU, idle window | 0.16 % of one core |
| CPU, behind a fullscreen game | the window stops drawing; only the audio threads run |
| DSP cost, Minion / heaviest voice | 0.09 % / 0.33 % of one core |
| The same with the noise gate closed | 0.11 % / 0.65 % of one core |
| Mic to cable, Minion | 29 ms on average; 17 ms with *Low latency* (11 to 27 ms) |
| Underruns in 30 s | 0 |

Where the 17 ms go (measured): 3 ms for the microphone's packet, 4.7 ms for the pitch
shifter to splice periods of your voice, and 9.3 ms of cushion between the microphone's
clock and the cable's plus the cable's own 2.67 ms periods. In shared mode the same USB
microphone only delivers 10 ms packets, which is what *Low latency* buys back by taking
it in exclusive mode (falling back to shared if Windows or the device refuses). The price
is that every other program loses that microphone meanwhile, so it is off by default.
Voices without a pitch shift skip the 4.7 ms.

Reproduce with:

```
cargo test --release realtime_factor -- --ignored --nocapture   # DSP cost per voice
cargo test --release gated_cost -- --ignored --nocapture        # the same between phrases
cargo test --release live_latency -- --ignored --nocapture      # needs a mic and VB-Cable
cargo test --release engine_periods -- --ignored --nocapture    # what each device supports
```

## How it works

```
microphone ──WASAPI (shared, 10 ms)────▶ gain + gate ─▶ voice recipe ─▶ soft limit
                                                         │
                               lock-free ring per output ◀┘
                                            │
                drift-compensating reader ──┴─▶ the cable (2.67 ms periods)
                                            └─▶ your headphones (optional)
```

- **Audio** is plain WASAPI. The microphone runs in shared mode, where Windows converts,
  or with *Low latency* in exclusive mode at the device's shortest period (float, int32 or
  int16, converted by Goby). Where a device supports it (VB-Cable does), the output uses
  `IAudioClient3` low-latency periods. Audio threads register with MMCSS as "Pro Audio".
- **Clock drift.** The microphone and the cable run on different clocks. Each output reads
  through a lock-free ring with a tiny resampler that nudges its rate (±0.5 % at most) to
  keep the ring at one microphone packet plus 2 ms, so latency stays flat for hours
  instead of creeping or clicking. When the output takes much smaller bites than the
  microphone delivers (10 ms shared-mode packets into the cable's 2.67 ms periods), half
  a packet plus one output period plus 2 ms is enough, and that is what it aims for.
- **Pitch** is a delay-line shifter with correlation-aligned jumps, essentially WSOLA
  inside a ring buffer. When the read tap runs out of room it jumps by one pitch period,
  found by cross-correlation, so the short crossfade adds instead of cancelling. Shifting
  down jumps as soon as one period fits, which keeps it as quick as shifting up.
- **The window** is Win32 drawn with Direct2D and DirectWrite into a DirectComposition
  swap chain, so the Mica backdrop shows through. Device pickers are real Win32 menus.
  Nothing redraws while the window is covered, minimised or showing a silent scope.
- **Routing.** The program list comes from the consent store Windows keeps for its
  microphone privacy page. Pinning a program uses the per-app audio policy behind the
  Volume mixer (`IAudioPolicyConfigFactory`), and the cable is renamed through
  `IPolicyConfig`, the interface behind the Sound control panel. Both are undocumented
  but have kept their layout across Windows 10 and 11.

## Inspiration

Inspired by [Clownfish Voice Changer](https://clownfish-translator.com/voicechanger/).

## Building

Needs Windows 10 or 11 and Rust 1.88 or newer. Mica needs Windows 11 22H2; older systems
get a solid background.

```
cargo build --release
cargo test --release
```

The binary lands in `target/release/goby.exe`. Settings live in `%APPDATA%\Goby\settings.ini`.

## Installing and uninstalling

Goby installs per user into `%LOCALAPPDATA%\Programs\Goby`, with a Start menu shortcut
and an entry under **Settings > Apps**. Running a newer download updates it in place.
Uninstall it from Settings > Apps: it closes Goby, hands the programs you chose back to
the default microphone, gives the cable its VB-Audio names back, and removes its
shortcuts, settings and folder.

For scripts and package managers:

```
goby-0.3.0-windows-x64.exe --install [--desktop]   # silent install, optional desktop shortcut
goby.exe --uninstall --quiet                        # silent uninstall
goby-0.3.0-windows-x64.exe --portable               # run without installing or asking
```

## Updates

Once a day, a little after it starts, Goby asks GitHub and got.az for the manifest of
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

## Author

Made by [Azer Sadykhzadeh](https://azer.one).

## License

Copyright © 2026 Azer Sadykhzadeh.

Goby is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the hope that
it will be useful, but without any warranty; see [LICENSE](LICENSE) for the full text.

In short: use it, share it and change it freely, but a changed version you give to others
has to come with its source under the same license.
