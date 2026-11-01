//! What the Windows Sound control panel does behind its buttons: renaming an endpoint, and
//! what the Volume mixer does for one program: choosing its microphone. Both interfaces are
//! undocumented, but the Sound panel and the mixer themselves depend on these exact layouts.

// The COM methods below keep their Windows names; order must match the vtable exactly.
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_DeviceDesc;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{EDataFlow, ERole, eCapture, eCommunications, eConsole, eMultimedia};
use windows::Win32::System::Com::StructuredStorage::{PROPVAR_CHANGE_FLAGS, PROPVARIANT, PropVariantChangeType};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::core::{GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, Interface, PCWSTR, Result, interface};

#[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *const c_void, mix: *const c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, period: *mut i64, minimum: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, period: *const i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, mode: *const c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, fx_store: i32, key: *const PROPERTYKEY, value: *mut PROPVARIANT) -> HRESULT;
    fn SetPropertyValue(
        &self,
        id: PCWSTR,
        fx_store: i32,
        key: *const PROPERTYKEY,
        value: *const PROPVARIANT,
    ) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
}

const POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

fn policy() -> Result<IPolicyConfig> {
    unsafe { CoCreateInstance(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
}

/// Renames an endpoint as the Sound panel does (the part before the driver name in
/// brackets). Writes to HKLM, so it only works elevated.
pub fn rename(id: &str, name: &str) -> Result<()> {
    let p = policy()?;
    let id = HSTRING::from(id);
    let bstr = PROPVARIANT::from(name);
    let mut text = PROPVARIANT::default();
    unsafe {
        // Endpoint names are stored as VT_LPWSTR, not the BSTR the conversion above makes.
        PropVariantChangeType(&mut text, &bstr, PROPVAR_CHANGE_FLAGS(0), VT_LPWSTR)?;
        p.SetPropertyValue(PCWSTR(id.as_ptr()), 0, &PKEY_Device_DeviceDesc, &text).ok()
    }
}

/// Entry point of `goby.exe --name-cable <render id> <capture id> [<render name> <capture name>]`,
/// run elevated: names the cable "Goby" / "Goby Mic", or whatever names are given.
pub fn name_cable(render: &str, capture: &str, names: (&str, &str)) -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    rename(render, names.0)?;
    rename(capture, names.1)
}

/// Per-app default devices: what Settings > Sound > Volume mixer sets for each program.
/// Activated through WinRT, with the vtable of current Windows 10 and 11. Windows remembers the
/// choice per program, so it survives restarts of the program (and of Goby).
#[interface("ab3d4648-e242-459f-b02f-541c70306324")]
unsafe trait IAudioPolicyConfigFactory: IUnknown {
    // IInspectable, spelled out: the macro only derives from IUnknown.
    fn __get_iids(&self) -> HRESULT;
    fn __get_runtime_class_name(&self) -> HRESULT;
    fn __get_trust_level(&self) -> HRESULT;
    fn __add_ctx_volume_change(&self) -> HRESULT;
    fn __remove_ctx_volume_changed(&self) -> HRESULT;
    fn __add_ringer_vibrate_state_changed(&self) -> HRESULT;
    fn __remove_ringer_vibrate_state_change(&self) -> HRESULT;
    fn __set_volume_group_gain_for_id(&self) -> HRESULT;
    fn __get_volume_group_gain_for_id(&self) -> HRESULT;
    fn __get_active_volume_group_for_endpoint_id(&self) -> HRESULT;
    fn __get_volume_groups_for_endpoint(&self) -> HRESULT;
    fn __get_current_volume_context(&self) -> HRESULT;
    fn __set_volume_group_mute_for_id(&self) -> HRESULT;
    fn __get_volume_group_mute_for_id(&self) -> HRESULT;
    fn __set_ringer_vibrate_state(&self) -> HRESULT;
    fn __get_ringer_vibrate_state(&self) -> HRESULT;
    fn __set_preferred_chat_application(&self) -> HRESULT;
    fn __reset_preferred_chat_application(&self) -> HRESULT;
    fn __get_preferred_chat_application(&self) -> HRESULT;
    fn __get_current_chat_applications(&self) -> HRESULT;
    fn __add_chat_context_changed(&self) -> HRESULT;
    fn __remove_chat_context_changed(&self) -> HRESULT;
    fn SetPersistedDefaultAudioEndpoint(&self, pid: u32, flow: EDataFlow, role: ERole, device: *mut c_void) -> HRESULT;
    fn GetPersistedDefaultAudioEndpoint(
        &self,
        pid: u32,
        flow: EDataFlow,
        role: ERole,
        device: *mut *mut c_void,
    ) -> HRESULT;
    fn ClearAllPersistedApplicationDefaultEndpoints(&self) -> HRESULT;
}

#[link(name = "combase", kind = "raw-dylib")]
unsafe extern "system" {
    fn RoGetActivationFactory(class: *mut c_void, iid: *const GUID, factory: *mut *mut c_void) -> HRESULT;
}

fn app_policy() -> Result<IAudioPolicyConfigFactory> {
    let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
    // Windows 11 and 10 21H2+ answer to the first IID, older Windows 10 to the second;
    // the vtable is the same.
    const DOWNLEVEL: GUID = GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258);
    let mut last = Ok(());
    for iid in [IAudioPolicyConfigFactory::IID, DOWNLEVEL] {
        let mut factory = std::ptr::null_mut();
        let hr = unsafe { RoGetActivationFactory(std::mem::transmute_copy(&class), &iid, &mut factory) };
        if hr.is_ok() && !factory.is_null() {
            return Ok(unsafe { IAudioPolicyConfigFactory::from_raw(factory) });
        }
        last = hr.ok();
    }
    last.and(Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_NOINTERFACE)))
}

/// The device-interface path the per-app API expects for a capture endpoint id.
pub fn capture_interface_path(endpoint_id: &str) -> String {
    format!("\\\\?\\SWD#MMDEVAPI#{endpoint_id}#{{2eef81be-33fa-4800-9670-1cd474972c3f}}")
}

/// Points one running program's microphone at `endpoint` (an endpoint id), or back at the
/// Windows default with `None`.
pub fn route_app(pid: u32, endpoint: Option<&str>) -> Result<()> {
    let factory = app_policy()?;
    let device = HSTRING::from(endpoint.map(capture_interface_path).unwrap_or_default());
    // Every role is attempted before reporting, so one refusal never leaves a half-set route.
    let mut result = Ok(());
    for role in [eConsole, eMultimedia, eCommunications] {
        let hr =
            unsafe { factory.SetPersistedDefaultAudioEndpoint(pid, eCapture, role, std::mem::transmute_copy(&device)) };
        result = result.and(hr.ok());
    }
    result
}

/// The microphone a running program is pinned to, as Windows reports it ("" = default).
pub fn app_route(pid: u32) -> Result<String> {
    let factory = app_policy()?;
    let mut raw = std::ptr::null_mut();
    unsafe {
        factory.GetPersistedDefaultAudioEndpoint(pid, eCapture, eConsole, &mut raw).ok()?;
        let device: HSTRING = std::mem::transmute(raw);
        Ok(device.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_path_matches_what_windows_stores() {
        let path = capture_interface_path("{0.0.1.00000000}.{8df855d0-fe30-4b2b-8d8d-53b0deb0c59c}");
        // Built from characters, not a copy of the same literal: a lost backslash once
        // slipped through both sides of this test.
        let prefix: String = ['\u{5c}', '\u{5c}', '?', '\u{5c}'].iter().collect();
        assert!(path.starts_with(&prefix), "{path}");
        assert_eq!(path.len(), 111, "Windows' own path for an endpoint is 111 characters");
        assert!(path.ends_with("#{2eef81be-33fa-4800-9670-1cd474972c3f}"));
    }

    /// Reads (never writes) the routing of the processes in GOBY_PIDS, or of this test.
    #[test]
    #[ignore = "talks to the Windows audio policy service"]
    fn reads_own_route() {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().unwrap() };
        let pids: Vec<u32> = std::env::var("GOBY_PIDS")
            .map(|v| v.split(',').filter_map(|p| p.parse().ok()).collect())
            .unwrap_or_else(|_| vec![std::process::id()]);
        for pid in pids {
            println!("{pid} records from: {:?}", app_route(pid));
        }
    }
}
