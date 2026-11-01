//! What the Windows Sound control panel does behind its buttons: renaming an endpoint and
//! making one the default. `IPolicyConfig` is undocumented but has kept this exact layout
//! since Windows 7; the Sound panel, EarTrumpet and SoundSwitch all rely on it.

// The COM methods below keep their Windows names; order must match the vtable exactly.
#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_DeviceDesc;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{ERole, eCommunications, eConsole, eMultimedia};
use windows::Win32::System::Com::StructuredStorage::{PROPVAR_CHANGE_FLAGS, PROPVARIANT, PropVariantChangeType};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::core::{GUID, HRESULT, HSTRING, IUnknown, IUnknown_Vtbl, PCWSTR, Result, interface};

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

/// Makes the endpoint the default for every role, so games without a device picker
/// (CS2) and voice chats alike pick it up. Needs no elevation.
pub fn set_default(id: &str) -> Result<()> {
    let p = policy()?;
    let id = HSTRING::from(id);
    for role in [eConsole, eMultimedia, eCommunications] {
        unsafe { p.SetDefaultEndpoint(PCWSTR(id.as_ptr()), role).ok()? };
    }
    Ok(())
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

/// Entry point of `goby.exe --name-cable <render id> <capture id>`, run elevated.
pub fn name_cable(render: &str, capture: &str) -> Result<()> {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    rename(render, "Goby")?;
    rename(capture, "Goby Mic")
}
