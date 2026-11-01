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

#[cfg(test)]
mod tests {
    use super::*;

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
