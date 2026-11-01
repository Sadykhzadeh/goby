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
use crate::mic::Mic;
use crate::ring::{Consumer, Producer, ring};
use crate::voices;

pub const RATE: u32 = 48_000;
/// Ring fill the outputs steer towards. Larger is safer, smaller is snappier.
const TARGET_MS: f64 = 12.0;
const MAX_BLOCK: usize = 4096;
pub const SCOPE_LEN: usize = 2048;

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

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
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
    channels: usize,
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
            Stream::finish(client, channels as usize)
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
            Stream::finish(client.cast()?, mix.nChannels as usize).map(Some)
        }
    }

    fn finish(client: IAudioClient, channels: usize) -> WinResult<Stream> {
        unsafe {
            let event = CreateEventW(None, false, false, None)?;
            client.SetEventHandle(event)?;
            let frames = client.GetBufferSize()?;
            Ok(Stream { client, event, frames, channels })
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
}

type Ready = mpsc::Sender<std::result::Result<(), String>>;

pub struct Engine {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Engine {
    /// `outputs[0]` is the cable; the rest (your headphones) do not count towards latency.
    pub fn start(input: &str, outputs: &[String], shared: Arc<Shared>) -> std::result::Result<Engine, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut engine = Engine { stop: stop.clone(), threads: Vec::new() };
        let mut producers = Vec::new();
        let (ready_tx, ready_rx) = mpsc::channel();

        for (i, id) in outputs.iter().enumerate() {
            let (prod, cons) = ring(RATE as usize);
            producers.push(prod);
            let (id, stop, shared, tx) = (id.clone(), stop.clone(), shared.clone(), ready_tx.clone());
            engine.threads.push(std::thread::spawn(move || render(&id, Reader::new(cons), i == 0, &stop, &shared, tx)));
        }
        let (id, stop2, shared2, tx) = (input.to_string(), stop.clone(), shared.clone(), ready_tx);
        engine.threads.push(std::thread::spawn(move || capture(&id, producers, &stop2, &shared2, tx)));

        for _ in 0..outputs.len() + 1 {
            match ready_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => return Err(e),
                Err(_) => return Err("Аудиоустройство не ответило".into()),
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
        AUDCLNT_E_DEVICE_IN_USE => "Устройство занято другой программой в монопольном режиме".into(),
        AUDCLNT_E_DEVICE_INVALIDATED => "Устройство отключено".into(),
        _ => format!("Ошибка звука: {}", e.message()),
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
        // Start from silence so the first period is not garbage.
        out.GetBuffer(s.frames)?;
        out.ReleaseBuffer(s.frames, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)?;
        s.client.Start()?;
        started();
        while !stop.load(Relaxed) {
            if !s.wait() {
                continue;
            }
            let n = s.frames - s.client.GetCurrentPadding()?;
            if n == 0 {
                continue;
            }
            let buf = std::slice::from_raw_parts_mut(out.GetBuffer(n)? as *mut f32, n as usize * s.channels);
            for frame in buf.chunks_exact_mut(s.channels) {
                frame.fill(reader.next());
            }
            out.ReleaseBuffer(n, 0)?;
            if measure {
                // Right after a write the device queue is full: ring + whole buffer.
                shared.queued_frames.store(reader.ring.len() as u32 + s.frames, Relaxed);
                shared.underruns.store(reader.underruns, Relaxed);
            }
        }
        Ok(())
    });
}

fn capture(id: &str, mut outs: Vec<Producer>, stop: &AtomicBool, shared: &Shared, ready: Ready) {
    run(shared, ready, |started| unsafe {
        let s = Stream::open(id, 1)?;
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
                let packet: &[f32] =
                    if silent { &[] } else { std::slice::from_raw_parts(data as *const f32, n as usize) };

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
                while done < n as usize {
                    let len = (n as usize - done).min(MAX_BLOCK);
                    let chunk = &mut block[..len];
                    if silent {
                        chunk.fill(0.0);
                    } else {
                        chunk.copy_from_slice(&packet[done..done + len]);
                    }
                    done += len;

                    mic.process(chunk, mic_gain, gate);
                    if mix > 0.0 {
                        dry[..len].copy_from_slice(chunk);
                    }
                    let start = shared.scope_pos.load(Relaxed);
                    for (i, s) in chunk.iter().enumerate() {
                        shared.scope_in[(start + i) % SCOPE_LEN].store(s.to_bits(), Relaxed);
                    }
                    voice.process(chunk);
                    if mix > 0.0 {
                        for (s, d) in chunk.iter_mut().zip(&dry[..len]) {
                            *s = *s * (1.0 - mix) + d * mix;
                        }
                    }
                    for (i, s) in chunk.iter().enumerate() {
                        shared.scope_out[(start + i) % SCOPE_LEN].store(s.to_bits(), Relaxed);
                    }
                    shared.scope_pos.store((start + len) % SCOPE_LEN, Relaxed);

                    for out in &mut outs {
                        // A full ring means that output stalled; dropping is the right call.
                        for s in chunk.iter() {
                            if !out.push(*s) {
                                break;
                            }
                        }
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

/// Pulls processed audio from the ring at the output's own clock.
pub struct Reader {
    ring: Consumer,
    frac: f64,
    s0: f32,
    s1: f32,
    target: f64,
    fill: f64,
    priming: bool,
    underruns: u32,
}

impl Reader {
    fn new(ring: Consumer) -> Self {
        let target = RATE as f64 * TARGET_MS / 1000.0;
        Self { ring, frac: 0.0, s0: 0.0, s1: 0.0, target, fill: target, priming: true, underruns: 0 }
    }

    fn next(&mut self) -> f32 {
        let avail = self.ring.len() as f64;
        if self.priming {
            if avail < self.target {
                return 0.0;
            }
            self.priming = false;
        }
        // After a long stall, throw away the backlog instead of playing it late.
        if avail > self.target * 4.0 {
            self.ring.skip((avail - self.target) as usize);
        }
        self.fill += (avail - self.fill) * 0.0005;
        // Proportional nudge, capped at ±0.5 %: inaudible, but enough for any real clock drift.
        let nudge = ((self.fill - self.target) / self.target * 0.02).clamp(-0.005, 0.005);
        self.frac += 1.0 + nudge;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            self.s0 = self.s1;
            match self.ring.pop() {
                Some(s) => self.s1 = s,
                None => {
                    self.priming = true;
                    self.underruns += 1;
                    return 0.0;
                }
            }
        }
        self.s0 + (self.s1 - self.s0) * self.frac as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_stays_near_target_under_clock_drift() {
        let target = (RATE as f64 * TARGET_MS / 1000.0) as usize;
        for drift in [-0.002, 0.0, 0.002] {
            // Feed RATE * (1 + drift) samples per second, read RATE per second, for 60 s.
            let (mut prod, cons) = ring(RATE as usize);
            let mut r = Reader::new(cons);
            let mut fed = 0.0f64;
            for _ in 0..60_000 {
                fed += RATE as f64 * (1.0 + drift) / 1000.0;
                while fed >= 1.0 {
                    prod.push(0.1);
                    fed -= 1.0;
                }
                for _ in 0..RATE / 1000 {
                    r.next();
                }
            }
            let fill = r.ring.len();
            assert!(fill.abs_diff(target) < target / 2, "drift {drift}: fill {fill}, target {target}");
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

    /// `cargo test --release live_latency -- --ignored --nocapture`
    #[test]
    #[ignore = "needs real audio devices"]
    fn live_latency() {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap() };
        let mic = default_endpoint(true).expect("no mic");
        let cable = list(false).into_iter().find(|e| e.name.contains("CABLE Input")).expect("no VB-Cable");
        let shared = Arc::new(Shared::default());
        shared.set(voices::index_of("minion").unwrap(), 0.0, 1.0);
        let engine = Engine::start(&mic.id, std::slice::from_ref(&cable.id), shared.clone()).expect("start");
        std::thread::sleep(Duration::from_secs(1));
        let mut samples = Vec::new();
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(100));
            samples.push(shared.latency_ms());
        }
        drop(engine);
        let avg = samples.iter().sum::<f32>() / samples.len() as f32;
        let (min, max) = samples.iter().fold((f32::MAX, 0f32), |(a, b), s| (a.min(*s), b.max(*s)));
        println!(
            "{} → {}: latency min {min:.1} avg {avg:.1} max {max:.1} ms, underruns {}, failed={}",
            mic.name,
            cable.name,
            shared.underruns.load(Relaxed),
            shared.failed.load(Relaxed)
        );
        assert!(!shared.failed.load(Relaxed));
    }
}
