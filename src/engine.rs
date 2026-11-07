//! Audio plumbing over raw WASAPI: real mic → `Voice` → virtual cable (and optionally headphones).
//!
//! Every stream asks Windows for 48 kHz float in shared mode and lets the audio engine
//! convert, so the DSP never deals with odd device formats. Mic and outputs still run on
//! their own hardware clocks, so each output reads through a lock-free ring and a tiny
//! resampler that nudges its rate to hold the ring at a fixed fill. That keeps latency
//! flat over an hour of play.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::Win32::System::Threading::{AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};
use windows::core::{HSTRING, Interface, PWSTR, Result as WinResult, w};

use crate::dsp::Voice;
use crate::host::{Endpoint, SCOPE_LEN};
use crate::lang::{tr, trf};
use crate::mic::Mic;
use crate::ring::{Producer, ring};
use crate::stream::{Reader, Sample, blend, tap, to_mono};
use crate::voices;

pub const RATE: u32 = 48_000;
const MAX_BLOCK: usize = 4096;

/// Lock-free knobs, meters and scope taps shared between the UI and the audio threads.
pub struct Shared {
    voice: AtomicU32,
    semitones: AtomicU32,
    gain: AtomicU32,
    /// Off passes the (gained, gated) mic through untouched; the chosen voice is kept.
    enabled: AtomicBool,
    mic_gain_db: AtomicU32,
    /// Gate threshold in dBFS; NaN means the gate is off.
    gate_db: AtomicU32,
    /// Share of the original voice mixed back in, 0..1.
    dry_mix: AtomicU32,
    pub failed: AtomicBool,
    scope_in: Box<[AtomicU32]>,
    scope_out: Box<[AtomicU32]>,
    scope_pos: AtomicUsize,
    /// Live latency parts, in frames: mic period, effect delay, and ring + device queue on the cable.
    mic_frames: AtomicU32,
    effect_frames: AtomicU32,
    queued_frames: AtomicU32,
    /// Frames in each mic packet, which the outputs size their rings by (0 = not known yet).
    mic_packet: AtomicU32,
    /// Whether the mic really runs in exclusive mode (it falls back to shared).
    pub mic_exclusive: AtomicBool,
    /// Times the cable ran dry and had to wait (each one is an audible gap).
    pub underruns: AtomicU32,
}

impl Default for Shared {
    fn default() -> Self {
        let taps = || (0..SCOPE_LEN).map(|_| AtomicU32::new(0)).collect();
        Self {
            voice: AtomicU32::new(0),
            semitones: AtomicU32::new(0),
            gain: AtomicU32::new(1f32.to_bits()),
            enabled: AtomicBool::new(true),
            mic_gain_db: AtomicU32::new(0f32.to_bits()),
            gate_db: AtomicU32::new(f32::NAN.to_bits()),
            dry_mix: AtomicU32::new(0f32.to_bits()),
            failed: AtomicBool::new(false),
            scope_in: taps(),
            scope_out: taps(),
            scope_pos: AtomicUsize::new(0),
            mic_frames: AtomicU32::new(0),
            effect_frames: AtomicU32::new(0),
            queued_frames: AtomicU32::new(0),
            mic_packet: AtomicU32::new(0),
            mic_exclusive: AtomicBool::new(false),
            underruns: AtomicU32::new(0),
        }
    }
}

impl Shared {
    /// Mic gain in dB, gate threshold in dBFS (`None` = off), and how much dry voice to keep.
    pub fn set_mic(&self, gain_db: f32, gate_db: Option<f32>, dry_mix: f32, enabled: bool) {
        self.mic_gain_db.store(gain_db.to_bits(), Relaxed);
        self.gate_db.store(gate_db.unwrap_or(f32::NAN).to_bits(), Relaxed);
        self.dry_mix.store(dry_mix.to_bits(), Relaxed);
        self.enabled.store(enabled, Relaxed);
    }

    /// `voice` indexes `voices::ALL`; `tone` shifts it in semitones.
    pub fn set(&self, voice: usize, tone: f32, gain: f32) {
        self.voice.store(voice as u32, Relaxed);
        self.semitones.store(tone.to_bits(), Relaxed);
        self.gain.store(gain.to_bits(), Relaxed);
    }

    /// Measured mic-to-cable latency in milliseconds; the mic period counts in full
    /// because a sample can arrive at the start of a packet.
    pub fn latency_ms(&self) -> f32 {
        let frames =
            self.mic_frames.load(Relaxed) + self.effect_frames.load(Relaxed) + self.queued_frames.load(Relaxed);
        frames as f32 * 1000.0 / RATE as f32
    }

    /// The most recent `SCOPE_LEN` samples before and after the effect, oldest first.
    pub fn scope(&self, raw: &mut [f32; SCOPE_LEN], voiced: &mut [f32; SCOPE_LEN]) {
        let pos = self.scope_pos.load(Relaxed);
        for i in 0..SCOPE_LEN {
            let j = (pos + i) % SCOPE_LEN;
            raw[i] = f32::from_bits(self.scope_in[j].load(Relaxed));
            voiced[i] = f32::from_bits(self.scope_out[j].load(Relaxed));
        }
    }
}

fn enumerator() -> WinResult<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

fn describe(dev: &IMMDevice) -> WinResult<Endpoint> {
    unsafe {
        let id = take_pwstr(dev.GetId()?);
        let store = dev.OpenPropertyStore(STGM_READ)?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName)?;
        let name = PropVariantToStringAlloc(&value).map(|p| take_pwstr(p)).unwrap_or_default();
        let _ = PropVariantClear(&mut value);
        Ok(Endpoint { id, name })
    }
}

/// VB-Cable endpoints carry the driver name in brackets, which survives renaming them.
fn is_cable(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("virtual cable") && !n.contains("16ch")
}

/// Every active endpoint of one direction, cable included.
fn list_all(capture: bool) -> Vec<Endpoint> {
    let flow = if capture { eCapture } else { eRender };
    let Ok(e) = enumerator() else { return Vec::new() };
    let Ok(all) = (unsafe { e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) }) else { return Vec::new() };
    let count = unsafe { all.GetCount() }.unwrap_or(0);
    (0..count).filter_map(|i| unsafe { all.Item(i) }.ok()).filter_map(|d| describe(&d).ok()).collect()
}

/// Who is recording right now: (process id, the capture endpoint it records from), for
/// every active session on every microphone. This is what a program really does, whatever
/// it was told to: one with its own microphone setting ignores the per-app default.
pub fn recording() -> Vec<(u32, Endpoint)> {
    let mut found = Vec::new();
    let Ok(e) = enumerator() else { return found };
    let Ok(all) = (unsafe { e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }) else { return found };
    for i in 0..unsafe { all.GetCount() }.unwrap_or(0) {
        let Ok(dev) = (unsafe { all.Item(i) }) else { continue };
        let Ok(ep) = describe(&dev) else { continue };
        let sessions =
            unsafe { dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None).and_then(|m| m.GetSessionEnumerator()) };
        let Ok(sessions) = sessions else { continue };
        for k in 0..unsafe { sessions.GetCount() }.unwrap_or(0) {
            let Ok(control) = (unsafe { sessions.GetSession(k) }) else { continue };
            let active = unsafe { control.GetState() }.is_ok_and(|s| s == AudioSessionStateActive);
            let pid = control.cast::<IAudioSessionControl2>().and_then(|c| unsafe { c.GetProcessId() });
            if let (true, Ok(pid)) = (active, pid) {
                found.push((pid, ep.clone()));
            }
        }
    }
    found
}

/// Endpoints to offer. The cable's own capture side would loop our output back in, so it is
/// never offered as a microphone.
pub fn list(capture: bool) -> Vec<Endpoint> {
    list_all(capture).into_iter().filter(|ep| !(capture && is_cable(&ep.name))).collect()
}

/// The virtual cable as (where Goby plays, what games record from).
pub fn cable() -> Option<(Endpoint, Endpoint)> {
    let play = list_all(false).into_iter().find(|e| is_cable(&e.name))?;
    let record = list_all(true).into_iter().find(|e| is_cable(&e.name))?;
    Some((play, record))
}

pub fn default_endpoint(capture: bool) -> Option<Endpoint> {
    let flow = if capture { eCapture } else { eRender };
    let dev = unsafe { enumerator().ok()?.GetDefaultAudioEndpoint(flow, eConsole) }.ok()?;
    describe(&dev).ok()
}

/// Integer or float PCM at 48 kHz, for exclusive mode where nothing converts for us.
fn pcm_format(channels: u16, sample: Sample) -> WAVEFORMATEX {
    let bits: u16 = if sample == Sample::I16 { 16 } else { 32 };
    WAVEFORMATEX {
        wFormatTag: if sample == Sample::F32 { 3 } else { 1 },
        nChannels: channels,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * (bits / 8 * channels) as u32,
        nBlockAlign: bits / 8 * channels,
        wBitsPerSample: bits,
        cbSize: 0,
    }
}

fn float_format(channels: u16) -> WAVEFORMATEX {
    WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: channels,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 4 * channels as u32,
        nBlockAlign: 4 * channels,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

struct Stream {
    client: IAudioClient,
    event: HANDLE,
    frames: u32,
    /// Frames the device takes or delivers per wake-up.
    period: u32,
    channels: usize,
    sample: Sample,
    exclusive: bool,
}

impl Stream {
    fn open(id: &str, channels: u16) -> WinResult<Stream> {
        unsafe {
            let dev = enumerator()?.GetDevice(&HSTRING::from(id))?;
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            // 10 ms: Windows rounds up to what the device can actually do.
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 100_000, 0, &float_format(channels), None)?;
            let mut period = 0i64;
            client.GetDevicePeriod(Some(&mut period), None)?;
            Stream::finish(client, (period * RATE as i64 / 10_000_000) as u32, channels as usize, Sample::F32)
        }
    }

    /// Exclusive mode at the device's shortest period: a USB mic that delivers 10 ms
    /// packets to the shared engine often manages 3 ms on its own. `None` when the device
    /// takes none of our formats; errors when it is busy or exclusive mode is disallowed.
    fn open_exclusive(id: &str) -> WinResult<Option<Stream>> {
        unsafe {
            let dev = enumerator()?.GetDevice(&HSTRING::from(id))?;
            let mut client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let (mut default, mut min) = (0i64, 0i64);
            client.GetDevicePeriod(Some(&mut default), Some(&mut min))?;
            let formats = [
                (1, Sample::F32),
                (1, Sample::I32),
                (1, Sample::I16),
                (2, Sample::F32),
                (2, Sample::I32),
                (2, Sample::I16),
            ];
            let Some((fmt, channels, sample)) = formats
                .into_iter()
                .map(|(ch, sample)| (pcm_format(ch, sample), ch, sample))
                .find(|(f, _, _)| client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, f, None).is_ok())
            else {
                return Ok(None);
            };
            let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
            if let Err(e) = client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, min, min, &fmt, None) {
                if e.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                    return Err(e);
                }
                // The driver wants a whole number of its own blocks: ask again with that size.
                let frames = client.GetBufferSize()?;
                let period = (10_000_000.0 * frames as f64 / RATE as f64).round() as i64;
                client = dev.Activate(CLSCTX_ALL, None)?;
                client.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, &fmt, None)?;
            }
            let frames = client.GetBufferSize()?;
            let mut s = Stream::finish(client, frames, channels as usize, sample)?;
            s.exclusive = true;
            Ok(Some(s))
        }
    }

    /// Shared mode with the smallest engine period the device allows (Windows 10+).
    /// VB-Cable goes down to 128 frames, 2.67 ms instead of the usual 10 ms. This only
    /// works in the device's own mix format, so it is used when that is 48 kHz float.
    fn open_low_latency(id: &str) -> WinResult<Option<Stream>> {
        unsafe {
            let dev = enumerator()?.GetDevice(&HSTRING::from(id))?;
            let client: IAudioClient3 = dev.Activate(CLSCTX_ALL, None)?;
            let fmt = client.GetMixFormat()?;
            let mix = *fmt;
            let float = mix.wBitsPerSample == 32 && (mix.wFormatTag == 3 || mix.wFormatTag == 0xFFFE);
            let (mut default, mut fundamental, mut min, mut max) = (0, 0, 0, 0);
            let periods = client.GetSharedModeEnginePeriod(fmt, &mut default, &mut fundamental, &mut min, &mut max);
            let ok = float && mix.nSamplesPerSec == RATE && periods.is_ok() && min < default;
            let result = if ok {
                client.InitializeSharedAudioStream(AUDCLNT_STREAMFLAGS_EVENTCALLBACK, min, fmt, None)
            } else {
                Ok(())
            };
            CoTaskMemFree(Some(fmt as _));
            if !ok {
                return Ok(None);
            }
            result?;
            Stream::finish(client.cast()?, min, mix.nChannels as usize, Sample::F32).map(Some)
        }
    }

    fn finish(client: IAudioClient, period: u32, channels: usize, sample: Sample) -> WinResult<Stream> {
        unsafe {
            let event = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(event)?;
            let frames = client.GetBufferSize()?;
            Ok(Stream { client, event, frames, period, channels, sample, exclusive: false })
        }
    }

    /// Waits for the device to ask for (or offer) more audio. False on timeout.
    fn wait(&self) -> bool {
        unsafe { WaitForSingleObject(self.event, 200) == WAIT_OBJECT_0 }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
    }
}

fn audio_thread_setup() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        // Lets the scheduler treat us like any other pro-audio app; harmless if it fails.
        let _ = AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut 0);
    }
    crate::dsp::flush_denormals();
}

type Ready = mpsc::Sender<std::result::Result<(), String>>;

pub struct Engine {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// `outputs[0]` is the cable; the rest (your headphones) do not count towards latency.
    /// `exclusive` asks for the mic in exclusive mode, falling back to shared if refused.
    pub fn start(
        input: &str,
        exclusive: bool,
        outputs: &[String],
        shared: Arc<Shared>,
    ) -> std::result::Result<Engine, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut engine = Engine { stop: stop.clone(), threads: Vec::new() };
        let mut producers = Vec::new();
        let (ready_tx, ready_rx) = mpsc::channel();

        for (i, id) in outputs.iter().enumerate() {
            let (prod, cons) = ring(RATE as usize);
            producers.push(prod);
            let (id, stop, shared, tx) = (id.clone(), stop.clone(), shared.clone(), ready_tx.clone());
            engine
                .threads
                .push(std::thread::spawn(move || render(&id, Reader::new(cons, RATE), i == 0, &stop, &shared, tx)));
        }
        let (id, stop2, shared2, tx) = (input.to_string(), stop.clone(), shared.clone(), ready_tx);
        engine.threads.push(std::thread::spawn(move || capture(&id, exclusive, producers, &stop2, &shared2, tx)));

        for _ in 0..outputs.len() + 1 {
            // Bluetooth headsets can take several seconds to open.
            match ready_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err(tr("error.no_answer").into()),
            }
        }
        Ok(engine)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn friendly(e: windows::core::Error) -> String {
    match e.code() {
        AUDCLNT_E_DEVICE_IN_USE => tr("error.in_use").into(),
        AUDCLNT_E_DEVICE_INVALIDATED => tr("error.unplugged").into(),
        _ => trf("error.audio", &[("message", &e.message())]),
    }
}

/// Runs `body` on an audio thread, reporting setup failures to `start` and later ones via `failed`.
fn run(shared: &Shared, ready: Ready, body: impl FnOnce(&mut dyn FnMut()) -> WinResult<()>) {
    audio_thread_setup();
    let mut reported = false;
    let result = body(&mut || {
        reported = true;
        let _ = ready.send(Ok(()));
    });
    if let Err(e) = result {
        if reported {
            shared.failed.store(true, Relaxed);
        } else {
            let _ = ready.send(Err(friendly(e)));
        }
    }
}

fn render(id: &str, mut reader: Reader, measure: bool, stop: &AtomicBool, shared: &Shared, ready: Ready) {
    run(shared, ready, |started| unsafe {
        let s = match Stream::open_low_latency(id) {
            Ok(Some(s)) => s,
            _ => Stream::open(id, 2)?,
        };
        let out: IAudioRenderClient = s.client.GetService()?;
        // Two periods queued: the one being mixed and the next. Anything short of a third
        // full period buys no extra time before a late wake-up glitches, only latency, and
        // VB-Cable's buffer holds 280 frames at a 128-frame period.
        let queue = s.frames.min(2 * s.period);
        // Start from silence so the first period is not garbage.
        out.GetBuffer(queue)?;
        out.ReleaseBuffer(queue, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)?;
        s.client.Start()?;
        started();
        while !stop.load(Relaxed) {
            if !s.wait() {
                continue;
            }
            reader.set_target(shared.mic_packet.load(Relaxed), s.period);
            let n = queue.saturating_sub(s.client.GetCurrentPadding()?);
            if n == 0 {
                continue;
            }
            let buf = std::slice::from_raw_parts_mut(out.GetBuffer(n)? as *mut f32, n as usize * s.channels);
            reader.render(buf, s.channels);
            out.ReleaseBuffer(n, 0)?;
            if measure {
                // Right after a write the device queue is full: ring + queue.
                shared.queued_frames.store(reader.queued() as u32 + queue, Relaxed);
                shared.underruns.store(reader.underruns(), Relaxed);
            }
        }
        Ok(())
    });
}

fn capture(id: &str, exclusive: bool, mut outs: Vec<Producer>, stop: &AtomicBool, shared: &Shared, ready: Ready) {
    run(shared, ready, |started| unsafe {
        let s = match exclusive.then(|| Stream::open_exclusive(id)) {
            Some(Ok(Some(s))) => s,
            _ => Stream::open(id, 1)?,
        };
        let is_exclusive = s.exclusive;
        shared.mic_exclusive.store(is_exclusive, Relaxed);
        // Shared mode hands over 10 ms packets whatever the buffer size is.
        let packet = if is_exclusive { s.frames } else { RATE / 100 };
        shared.mic_packet.store(packet, Relaxed);
        let mut input = vec![0.0f32; s.frames.max(MAX_BLOCK as u32) as usize * 2];
        let cap: IAudioCaptureClient = s.client.GetService()?;
        let mut voice = Voice::new(RATE);
        let mut mic = Mic::new(RATE);
        let mut block = vec![0.0f32; MAX_BLOCK];
        let mut dry = vec![0.0f32; MAX_BLOCK];
        s.client.Start()?;
        started();
        while !stop.load(Relaxed) {
            if !s.wait() {
                continue;
            }
            while cap.GetNextPacketSize()? > 0 {
                let (mut data, mut n, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                cap.GetBuffer(&mut data, &mut n, &mut flags, None, None)?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                let frames = (n as usize).min(input.len());
                if silent {
                    input[..frames].fill(0.0);
                } else {
                    to_mono(data, frames, s.channels, s.sample, &mut input);
                }

                let enabled = shared.enabled.load(Relaxed);
                let chosen = if enabled { shared.voice.load(Relaxed) as usize } else { 0 };
                let def = voices::ALL.get(chosen).unwrap_or(&voices::ALL[0]);
                let semis = if enabled { f32::from_bits(shared.semitones.load(Relaxed)) } else { 0.0 };
                let mic_gain = f32::from_bits(shared.mic_gain_db.load(Relaxed));
                let gate = Some(f32::from_bits(shared.gate_db.load(Relaxed))).filter(|g| !g.is_nan());
                let mix = if enabled { f32::from_bits(shared.dry_mix.load(Relaxed)) } else { 0.0 };
                let gain = f32::from_bits(shared.gain.load(Relaxed));
                voice.set(&def.recipe, semis, gain);

                let mut done = 0;
                while done < frames {
                    let len = (frames - done).min(MAX_BLOCK);
                    let chunk = &mut block[..len];
                    chunk.copy_from_slice(&input[done..done + len]);
                    done += len;

                    mic.process(chunk, mic_gain, gate);
                    if mix > 0.0 {
                        dry[..len].copy_from_slice(chunk);
                    }
                    let start = shared.scope_pos.load(Relaxed);
                    tap(&shared.scope_in, start, chunk);
                    voice.process(chunk);
                    if mix > 0.0 {
                        blend(chunk, &dry[..len], mix);
                    }
                    tap(&shared.scope_out, start, chunk);
                    shared.scope_pos.store((start + len) % SCOPE_LEN, Relaxed);

                    for out in &mut outs {
                        // A full ring means that output stalled; dropping is the right call.
                        out.push_all(chunk);
                    }
                }
                cap.ReleaseBuffer(n)?;
                shared.mic_frames.store(n, Relaxed);
                shared.effect_frames.store(voice.delay() as u32, Relaxed);
            }
        }
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_threads_flush_denormals() {
        let tiny = std::thread::spawn(|| {
            audio_thread_setup();
            std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5)
        });
        assert_eq!(tiny.join().unwrap(), 0.0);
    }

    #[test]
    fn pcm_formats_are_self_consistent() {
        for (ch, sample, bits, tag) in [(1, Sample::I16, 16, 1), (2, Sample::I32, 32, 1), (1, Sample::F32, 32, 3)] {
            let f = pcm_format(ch, sample);
            let (format_tag, bits_per_sample, channels) = (f.wFormatTag, f.wBitsPerSample, f.nChannels);
            assert_eq!((format_tag, bits_per_sample, channels), (tag, bits, ch));
            // WAVEFORMATEX is packed: copy fields out before comparing.
            let (align, rate) = (f.nBlockAlign, f.nAvgBytesPerSec);
            assert_eq!(align, bits / 8 * ch);
            assert_eq!(rate, RATE * align as u32);
        }
    }

    #[test]
    #[ignore = "needs real audio devices"]
    fn lists_devices() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        for ep in list(true).iter().chain(list(false).iter()) {
            println!("{} | {}", ep.name, ep.id);
        }
        println!("default mic: {:?}", default_endpoint(true));
        println!("cable: {:?}", cable());
    }
}

#[cfg(test)]
mod probe {
    use super::*;

    /// `cargo test --release engine_periods -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real audio devices"]
    fn engine_periods() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        for ep in list(true).into_iter().chain(list(false)) {
            let r = (|| unsafe {
                let dev = enumerator()?.GetDevice(&HSTRING::from(ep.id.as_str()))?;
                let c: IAudioClient3 = dev.Activate(CLSCTX_ALL, None)?;
                let fmt = c.GetMixFormat()?;
                let (mut def, mut fund, mut min, mut max) = (0, 0, 0, 0);
                c.GetSharedModeEnginePeriod(fmt, &mut def, &mut fund, &mut min, &mut max)?;
                let rate = (*fmt).nSamplesPerSec;
                CoTaskMemFree(Some(fmt as _));
                WinResult::Ok(format!(
                    "{rate} Hz: default {def} min {min} frames ({:.2} ms min)",
                    min as f32 * 1000.0 / rate as f32
                ))
            })();
            println!("{:<45} {}", ep.name, r.unwrap_or_else(|e| e.to_string()));
        }
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Lists who records from which microphone right now; reads only.
    /// `cargo test --release who_records -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real audio devices"]
    fn who_records() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let names = crate::apps::processes();
        for (pid, ep) in recording() {
            let exe = names.iter().find(|(_, p)| *p == pid).map_or("?", |(n, _)| n.as_str());
            println!("{exe} ({pid}) records from {}", ep.name);
        }
    }

    /// `cargo test --release live_latency -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real audio devices"]
    fn live_latency() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let saved = crate::settings::Settings::load().input;
        let mics = list(true);
        let mic = mics.iter().find(|e| e.id == saved).or(mics.first()).expect("no mic").clone();
        let (cable, _) = cable().expect("no VB-Cable");
        let shared = Arc::new(Shared::default());
        shared.set(voices::index_of("minion").unwrap(), 0.0, 1.0);
        let exclusive = std::env::var("GOBY_SHARED").is_err();
        let engine = Engine::start(&mic.id, exclusive, std::slice::from_ref(&cable.id), shared.clone()).expect("start");
        std::thread::sleep(Duration::from_secs(1));
        let mut samples = Vec::new();
        let mut parts = Vec::new();
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(100));
            samples.push(shared.latency_ms());
            parts.push([&shared.mic_frames, &shared.effect_frames, &shared.queued_frames].map(|a| a.load(Relaxed)));
        }
        drop(engine);
        let avg = samples.iter().sum::<f32>() / samples.len() as f32;
        let (min, max) = samples.iter().fold((f32::MAX, 0f32), |(a, b), s| (a.min(*s), b.max(*s)));
        println!(
            "{} → {}: latency min {min:.1} avg {avg:.1} max {max:.1} ms, underruns {}, exclusive={}, failed={}",
            mic.name,
            cable.name,
            shared.underruns.load(Relaxed),
            shared.mic_exclusive.load(Relaxed),
            shared.failed.load(Relaxed)
        );
        let avg_ms = |k: usize| {
            parts.iter().map(|p: &[u32; 3]| p[k] as f32).sum::<f32>() / parts.len() as f32 / RATE as f32 * 1000.0
        };
        println!(
            "  mic packet {:.1} ms, effect {:.1} ms, ring + cable queue {:.1} ms",
            avg_ms(0),
            avg_ms(1),
            avg_ms(2)
        );
        assert!(!shared.failed.load(Relaxed));
    }
}

#[cfg(test)]
mod exclusive_probe {
    use super::*;

    /// `cargo test --release exclusive_formats -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real audio devices"]
    fn exclusive_formats() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        for ep in list(true) {
            let r = (|| unsafe {
                let dev = enumerator()?.GetDevice(&HSTRING::from(ep.id.as_str()))?;
                let c: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
                let (mut def, mut min) = (0i64, 0i64);
                c.GetDevicePeriod(Some(&mut def), Some(&mut min))?;
                let mut ok = Vec::new();
                for ch in [1u16, 2] {
                    for (tag, bits) in [(1u16, 16u16), (1, 24), (1, 32), (3, 32)] {
                        let f = WAVEFORMATEX {
                            wFormatTag: tag,
                            nChannels: ch,
                            nSamplesPerSec: RATE,
                            nAvgBytesPerSec: RATE * (bits / 8 * ch) as u32,
                            nBlockAlign: bits / 8 * ch,
                            wBitsPerSample: bits,
                            cbSize: 0,
                        };
                        if c.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &f, None).is_ok() {
                            ok.push(format!("{ch}ch {}{bits}", if tag == 3 { "f" } else { "i" }));
                        }
                    }
                }
                WinResult::Ok(format!(
                    "period default {:.2} ms, min {:.2} ms; exclusive: {:?}",
                    def as f64 / 1e4,
                    min as f64 / 1e4,
                    ok
                ))
            })();
            println!("{:<34} {}", ep.name, r.unwrap_or_else(|e| e.to_string()));
        }
    }
}
