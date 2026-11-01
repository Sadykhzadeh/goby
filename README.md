# Goby

A small, fast voice changer for Windows. It takes your real microphone, turns you into a
minion, a demon, a robot, a walkie-talkie or one of 61 voices, and hands the result to any
game or chat app through a virtual audio cable.

One ~400 KB executable. No installer, no background service, no driver of its own, no
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
2. Run `goby.exe`. It picks your microphone and sends the changed voice into the cable.
3. Under **Output**, press **Mic for games**. That makes the cable the Windows default
   microphone, which is what CS2 uses (it has no microphone picker of its own). Apps
   that do have a picker can simply select it.
4. Optional: **Name the cable Goby**. After a UAC prompt the cable shows up as
   "Goby Mic" everywhere instead of "CABLE Output".

Goby never offers the cable as its own microphone, so it cannot feed itself back in.

Microphone settings: gain, a noise gate that mutes keyboard clicks and room noise
between phrases, how much of your original voice to mix back in, an effect switch that
keeps the chosen voice, and *Hear myself* for your headphones. Arrow keys walk the
gallery; double-click a slider to reset it.

## Numbers

Release build on Windows 11 with a USB microphone (fifine) and VB-Cable:

| | |
|---|---|
| Executable | 414 KB |
| Dependencies | `windows`, `windows-core`, `windows-numerics` (Microsoft's own bindings) |
| CPU, idle window | 0.16 % of one core |
| CPU, behind a fullscreen game | the window stops drawing; only the audio threads run |
| DSP cost, Minion / heaviest voice | 0.18 % / 1.05 % of one core |
| Mic to cable, Minion | 22 to 43 ms, 31 ms on average |
| Underruns in 30 s | 0 |

Where the latency goes: the USB microphone delivers audio in 10 ms packets (the
smallest Windows allows for it in shared mode), the pitch shifter needs 4 to 9 ms to
splice periods of your voice, and a 12 ms cushion absorbs the difference between the
microphone's and the cable's clocks. The cable side runs at 2.67 ms periods.

Reproduce with:

```
cargo test --release realtime_factor -- --ignored --nocapture   # DSP cost per voice
cargo test --release live_latency -- --ignored --nocapture      # needs a mic and VB-Cable
cargo test --release engine_periods -- --ignored --nocapture    # what each device supports
```

## How it works

```
microphone ──WASAPI──▶ gain + gate ─▶ voice recipe ─▶ soft limit
                                                         │
                               lock-free ring per output ◀┘
                                            │
                drift-compensating reader ──┴─▶ the cable (2.67 ms periods)
                                            └─▶ your headphones (optional)
```

- **Audio** is plain WASAPI in shared mode. Every stream asks Windows for 48 kHz float and
  lets the audio engine convert, so the effects never see odd device formats. Where a
  device supports it (VB-Cable does), the output uses `IAudioClient3` low-latency periods.
  Audio threads register with MMCSS as "Pro Audio".
- **Clock drift.** The microphone and the cable run on different clocks. Each output reads
  through a lock-free ring with a tiny resampler that nudges its rate (±0.5 % at most) to
  keep the ring at 12 ms, so latency stays flat for hours instead of creeping or clicking.
- **Pitch** is a delay-line shifter with correlation-aligned jumps, essentially WSOLA
  inside a ring buffer. When the read tap runs out of room it jumps by one pitch period,
  found by cross-correlation, so the short crossfade adds instead of cancelling. Shifting
  down jumps as soon as one period fits, which keeps it as quick as shifting up.
- **The window** is Win32 drawn with Direct2D and DirectWrite into a DirectComposition
  swap chain, so the Mica backdrop shows through. Device pickers are real Win32 menus.
  Nothing redraws while the window is covered, minimised or showing a silent scope.
- **The cable** is made the default microphone, and optionally renamed, through
  `IPolicyConfig`, the interface behind the Sound control panel.

## Inspiration

Goby was inspired by [Clownfish Voice Changer](https://clownfish-translator.com/voicechanger/).
Clownfish takes a different route: it installs an Audio Processing Object into the Windows
audio engine on the microphone itself, so every app hears the effect with no setup. That
works without a cable, but it means rewriting protected registry keys and asking Windows to
load unsigned code into `audiodg.exe`, and it tends to break after Windows updates. Goby
stays an ordinary program you can close at any time; it never touches the system audio
pipeline.

## Building

Needs Windows 10 or 11 and Rust 1.88 or newer. Mica needs Windows 11 22H2; older systems
get a solid background.

```
cargo build --release
cargo test --release
```

The binary lands in `target/release/goby.exe`. Settings live in `%APPDATA%\Goby\settings.ini`.

## License

MIT, see [LICENSE](LICENSE).
