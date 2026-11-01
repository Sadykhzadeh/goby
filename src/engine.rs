//! Audio plumbing over raw WASAPI: real mic → `Voice` → virtual cable (and optionally headphones).
//!
//! Every stream asks Windows for 48 kHz float in shared mode and lets the audio engine
//! convert, so the DSP never deals with odd device formats. Mic and outputs still run on
//! their own hardware clocks, so each output reads through a lock-free ring and a tiny
//! resampler that nudges its rate to hold the ring at a fixed fill. That keeps latency
//! flat over an hour of play.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, STGM_READ,
};
use windows::core::{PWSTR, Result as WinResult};

use crate::ring::Consumer;

pub const RATE: u32 = 48_000;
/// Ring fill the outputs steer towards. Larger is safer, smaller is snappier.
const TARGET_MS: f64 = 12.0;

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

/// The cable's own capture side would loop our output back in, so it is never offered as a mic.
fn is_cable_capture(name: &str) -> bool {
    name.to_ascii_lowercase().contains("cable output")
}

pub fn list(capture: bool) -> Vec<Endpoint> {
    let flow = if capture { eCapture } else { eRender };
    let Ok(e) = enumerator() else { return Vec::new() };
    let Ok(all) = (unsafe { e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) }) else { return Vec::new() };
    let count = unsafe { all.GetCount() }.unwrap_or(0);
    (0..count)
        .filter_map(|i| unsafe { all.Item(i) }.ok())
        .filter_map(|d| describe(&d).ok())
        .filter(|ep| !(capture && is_cable_capture(&ep.name)))
        .collect()
}

pub fn default_endpoint(capture: bool) -> Option<Endpoint> {
    let flow = if capture { eCapture } else { eRender };
    let dev = unsafe { enumerator().ok()?.GetDefaultAudioEndpoint(flow, eConsole) }.ok()?;
    describe(&dev).ok()
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
}

impl Reader {
    fn new(ring: Consumer) -> Self {
        let target = RATE as f64 * TARGET_MS / 1000.0;
        Self { ring, frac: 0.0, s0: 0.0, s1: 0.0, target, fill: target, priming: true }
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
    use crate::ring::ring;

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
    }
}
