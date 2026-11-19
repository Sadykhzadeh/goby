# Troubleshooting

The problems people hit most, what Goby shows for each, and what to check. The main window
and the Programs page say most of this in words; the steps below follow the same order.

## Nothing is heard in the other program

1. **The cable is missing.** The main window says "No virtual cable: open Programs". Install
   the free [VB-Audio Virtual Cable](https://vb-audio.com/Cable/) from that page: unzip it,
   run `VBCABLE_Setup_x64.exe` as administrator and press Install Driver. Goby connects to it
   by itself; if that takes long, restart Windows.
2. **The program is not chosen.** Only the programs on the Programs page hear the voice. Press
   **+** next to the program. Until one is chosen, the main window says "Nobody hears Goby yet".
3. **The effect is off.** With the effect off your own voice goes through unchanged, and the
   main window says so. Press Ctrl+Alt+G to switch it on.
4. **The program has its own microphone setting.** A program such as Telegram can keep its own
   choice of microphone. Set it to **Default** once. The Programs page says "Records from
   ..., not Goby" when it sees this.
5. **The program has not opened the microphone yet.** The Programs page says "Hears Goby once
   it starts" for a program that is not running, and "Hears Goby once it opens the mic" for one
   that is running but has not opened the microphone. Start the call or the recording, or rejoin it.
6. **Another program holds the device.** Goby can report "Another program holds the
   device exclusively". Close that program, then try again.

## Echo

- **Your own voice comes back through the microphone.** *Hear myself* plays your voice into
  your headphones. Through speakers, the microphone picks it up again and the other side hears
  an echo. Turn *Hear myself* off, or wear headphones.
- **The voice has an echo or a room in it.** Some voices add one on purpose. Seven have an echo:
  Tipsy, Announcer, Sports commentator, Hologram, Stadium, Canyon and Karaoke. Twenty-four have
  a room (reverb), from Giant and Pirate to Cave, Cathedral and Opera. Pick a voice without
  either, such as Minion, Kid, Robot or Radio, to hear without it.

## Delay

- Goby adds about 28 ms between your mouth and the other program, or about 17 ms with
  *Low latency* (Settings > Advanced). The program you talk through adds its own buffering
  on top of that, so the total is longer than Goby's share alone.
- *Low latency* saves about 11 ms by taking the microphone in exclusive mode. While Goby runs,
  other programs cannot record from that microphone. If Windows or the device refuses, the
  setting says "The mic did not allow it" and Goby keeps the shared mode.

## The start of words is cut off, or the voice is too quiet or distorted

- **Word starts clipped:** the noise gate closed too early. Lower *Gate threshold* under
  Settings > Advanced, or switch *Noise gate* off in Settings.
- **Too loud or distorted:** the microphone gain is too high. Lower *Gain* under
  Settings > Advanced. Goby eases peaks towards full scale, so very loud input is
  compressed rather than clipped.
- **Too quiet:** raise *Volume* on the main window, or *Gain* under Settings > Advanced, a little at a time.

## Goby reports a device error

- "The audio device did not respond" and "The device was unplugged": check the microphone's
  connection, then choose it again under Settings > Microphone > Device.
- "Audio error" followed by a message: the message is Windows' own; search it, or open an issue
  with the text and the version from Settings.
