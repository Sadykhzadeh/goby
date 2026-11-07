# How Goby works

Measured numbers first, then the audio path, the pitch shifter, the window and the routing.

## Numbers

Release build on Windows 11 with a USB microphone (fifine) and VB-Cable:

| | |
|---|---|
| Executable | 522 KB, icons, installer, tray and updates included |
| Dependencies | `windows`, `windows-core`, `windows-numerics` (Microsoft's own bindings) |
| CPU, idle window | 0.16 % of one core |
| CPU, behind a fullscreen game | the window stops drawing; only the audio threads run |
| DSP cost, Minion / heaviest voice | 0.09 % / 0.33 % of one core |
| The same with the noise gate closed | 0.11 % / 0.65 % of one core |
| Mic to cable, Minion | 28 ms on average; 17 ms with *Low latency* (11 to 27 ms) |
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
- **The same window on the website.** Everything the window shows and does (`src/ui`)
  draws through a small `Canvas` interface and asks the system for devices, programs and
  settings through a `Host`. The app answers with Direct2D and Win32; the website builds
  the same files for WebAssembly (`site/dsp-wasm`) and answers with a canvas and sample
  devices, so goby.azer.one shows the real window rather than a copy of it.
- **Routing.** The program list comes from the consent store Windows keeps for its
  microphone privacy page. Pinning a program uses the per-app audio policy behind the
  Volume mixer (`IAudioPolicyConfigFactory`), and the cable is renamed through
  `IPolicyConfig`, the interface behind the Sound control panel. Both are undocumented
  but have kept their layout across Windows 10 and 11.
