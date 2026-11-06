# Goby

A small, fast voice changer for Windows. It takes your real microphone, turns you into a
minion, a demon, a robot, a walkie-talkie or one of 61 voices, and hands the result to any
game or chat app through a virtual audio cable.

One ~600 KB executable that installs itself for your user, no administrator rights
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

1. Install [VB-Audio Virtual Cable](https://vb-audio.com/Cable/) (free).
2. Run the downloaded `goby-0.2.0-windows-x64.exe` and choose **Install Goby** (or
   **Just run it**). On the first start Windows asks once for permission to rename the
   cable, so it shows up as **Goby Mic** everywhere instead of "CABLE Output".
3. Open **Settings** (the gear at the bottom left); under **Who hears Goby**, press **Add a program** and pick CS2, Discord or OBS from
   the menu. It lists every program that has used your microphone (the same list Windows
   keeps for its privacy page); **Other program…** opens the usual file picker. Goby pins
   only the chosen programs to Goby Mic, the way the Volume mixer's per-app settings do.
   Everything else keeps your real microphone and nothing system-wide changes. The ×
   next to a program hands it back.

CS2 has no microphone picker of its own, which is exactly why the per-program switch is
there. Goby never offers the cable as its own microphone, so it cannot feed itself back in.

The main window holds what you change while playing: the voice, the effect switch (it
keeps the chosen voice), pitch, volume and *Hear myself* for your headphones. Everything
you set once lives in **Settings**: the programs that hear Goby, the microphone with its
gain, a noise gate that mutes keyboard clicks and room noise between phrases and *Low
latency*, which takes the microphone in exclusive mode, the output, how much of your
original voice to mix back in, and the tray, startup and hotkey switches. Arrow keys
walk the gallery, Esc leaves Settings, and a double-click resets a slider.

Hotkeys work from inside a game: **Ctrl+Alt+G** switches the effect on and off, and
**Ctrl+Alt+Page Down** / **Page Up** step to the next or previous voice (which also
switches the effect back on). Settings shows when another program already holds one of
them, and can turn them off.

Closing the window leaves Goby running in the notification area, so your voice stays on
in the game. Click the icon to bring the window back; right-click it to switch the effect
off or to quit. *Keep running in the tray* under Settings turns this off, and then closing
the window quits. `goby.exe --tray` starts with only the icon, and *Start with Windows*
(off until you turn it on) does that at sign-in. Uninstalling removes it from startup.

## Numbers

Release build on Windows 11 with a USB microphone (fifine) and VB-Cable:

| | |
|---|---|
| Executable | 620 KB, icons, installer and tray included |
| Dependencies | `windows`, `windows-core`, `windows-numerics` (Microsoft's own bindings) |
| CPU, idle window | 0.16 % of one core |
| CPU, behind a fullscreen game | the window stops drawing; only the audio threads run |
| DSP cost, Minion / heaviest voice | 0.18 % / 1.05 % of one core |
| Mic to cable, Minion | 11 to 27 ms, 17 ms on average (31 ms in shared mode) |
| Underruns in 30 s | 0 |

Where the 17 ms go (measured): 3 ms for the microphone's packet, 4.7 ms for the pitch
shifter to splice periods of your voice, and 9.3 ms of cushion between the microphone's
clock and the cable's plus the cable's own 2.67 ms periods. In shared mode the same USB
microphone only delivers 10 ms packets, which is why *Low latency* takes it in exclusive
mode and falls back to shared if Windows or the device refuses. Voices without a pitch
shift skip the 4.7 ms.

Reproduce with:

```
cargo test --release realtime_factor -- --ignored --nocapture   # DSP cost per voice
cargo test --release live_latency -- --ignored --nocapture      # needs a mic and VB-Cable
cargo test --release engine_periods -- --ignored --nocapture    # what each device supports
```

## How it works

```
microphone ──WASAPI (exclusive, 3 ms)──▶ gain + gate ─▶ voice recipe ─▶ soft limit
                                                         │
                               lock-free ring per output ◀┘
                                            │
                drift-compensating reader ──┴─▶ the cable (2.67 ms periods)
                                            └─▶ your headphones (optional)
```

- **Audio** is plain WASAPI. The microphone runs in exclusive mode at the device's
  shortest period (float, int32 or int16, converted by Goby), or in shared mode where
  Windows converts. Where a device supports it (VB-Cable does), the output uses
  `IAudioClient3` low-latency periods. Audio threads register with MMCSS as "Pro Audio".
- **Clock drift.** The microphone and the cable run on different clocks. Each output reads
  through a lock-free ring with a tiny resampler that nudges its rate (±0.5 % at most) to
  keep the ring at one microphone packet plus 2 ms, so latency stays flat for hours
  instead of creeping or clicking.
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
goby-0.2.0-windows-x64.exe --install [--desktop]   # silent install, optional desktop shortcut
goby.exe --uninstall --quiet                        # silent uninstall
goby-0.2.0-windows-x64.exe --portable               # run without installing or asking
```

## Author

Made by [Azer Sadykhzadeh](https://azer.one).

## License

MIT, see [LICENSE](LICENSE).
