//! Getting VB-Cable onto a computer that has none. Goby fetches the author's own package
//! from vb-audio.com and opens its setup; pressing Install Driver there stays with the
//! person, as its licence asks. Anything unexpected falls back to the download page.

use windows::Win32::Foundation::HWND;
use windows::Win32::Security::Cryptography::{CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW};
use windows::Win32::Security::WinTrust::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW};
use windows::Win32::UI::Shell::FOLDERID_System;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
use windows::core::{HSTRING, PCWSTR, w};

use crate::{files, install, update};

const PACK: &str = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack46.zip";
pub const PAGE: PCWSTR = w!("https://vb-audio.com/Cable/");
/// The name on VB-Audio's code-signing certificate.
const SIGNER: &str = "BUREL VINCENT";

/// The driver is on this computer, even when its devices are not up yet (no restart since
/// setup, or turned off in Sound settings).
pub fn installed() -> bool {
    let mut key = HKEY::default();
    let path = w!(r"SYSTEM\CurrentControlSet\Services\VBAudioVACMME");
    unsafe {
        let found = RegOpenKeyExW(HKEY_LOCAL_MACHINE, path, None, KEY_READ, &mut key).is_ok();
        if found {
            let _ = RegCloseKey(key);
        }
        found
    }
}

/// Downloads and unpacks the package and opens its setup behind the UAC prompt, on a
/// thread of its own. The files live under `update\`, which the next start clears.
pub fn install() {
    crate::spawn(Box::new(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        if fetch().is_none() {
            install::launch(PAGE, "", None, SW_SHOWNORMAL);
        }
    }));
}

fn fetch() -> Option<()> {
    let dir = format!("{}\\update\\vbcable", crate::settings::Settings::dir()?);
    files::create_dirs(&dir).ok()?;
    let zip = format!("{dir}\\pack.zip");
    if !files::write(&zip, &update::get(PACK, 16 << 20)?) {
        return None;
    }
    // Windows' own tar reads zip files, so no unpacker comes along in Goby.
    let tar = HSTRING::from(install::known_folder(&FOLDERID_System)? + "\\tar.exe");
    install::shell(w!("open"), &tar, &format!("-xf \"{zip}\" -C \"{dir}\""), None, SW_HIDE, true);
    let setup = format!("{dir}\\VBCABLE_Setup_x64.exe");
    if !signed_by_vb_audio(&setup) {
        return None;
    }
    let (setup, dir) = (HSTRING::from(setup), HSTRING::from(dir));
    install::shell(w!("runas"), &setup, "", Some(&dir), SW_SHOWNORMAL, false).then_some(())
}

/// A valid Authenticode signature whose signer is VB-Audio's author.
fn signed_by_vb_audio(path: &str) -> bool {
    let path = HSTRING::from(path);
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(path.as_ptr()),
        ..Default::default()
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    unsafe {
        let mut ok = WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut WINTRUST_DATA as _) == 0;
        if ok {
            let signer = WTHelperGetProvSignerFromChain(WTHelperProvDataFromStateData(data.hWVTStateData), 0, false, 0);
            ok = !signer.is_null() && (*signer).csCertChain > 0 && {
                let mut name = [0u16; 128];
                let cert = (*(*signer).pasCertChain).pCert;
                CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut name));
                name.iter().copied().take(SIGNER.len()).eq(SIGNER.bytes().map(u16::from))
            };
        }
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, &mut data as *mut WINTRUST_DATA as _);
        ok
    }
}

#[cfg(test)]
mod tests {
    /// Needs the internet: the real package downloads, unpacks and carries VB-Audio's signature.
    #[test]
    #[ignore]
    fn live_package_is_signed_by_vb_audio() {
        let dir = std::env::temp_dir().join("goby-cable-test");
        let _ = std::fs::create_dir_all(&dir);
        let zip = dir.join("pack.zip");
        std::fs::write(&zip, super::update::get(super::PACK, 16 << 20).expect("download")).unwrap();
        let tar = r"C:\Windows\System32\tar.exe";
        let status = std::process::Command::new(tar).arg("-xf").arg(&zip).arg("-C").arg(&dir).status().unwrap();
        assert!(status.success());
        let setup = dir.join("VBCABLE_Setup_x64.exe");
        assert!(super::signed_by_vb_audio(setup.to_str().unwrap()));
        // Any other signer is refused, even a valid one (Git for Windows' own signature).
        let other = r"C:\Program Files\Git\cmd\git.exe";
        assert!(!std::path::Path::new(other).exists() || !super::signed_by_vb_audio(other));
        // So is the right file with one byte changed.
        let mut bytes = std::fs::read(&setup).unwrap();
        bytes[0x400] ^= 1;
        let tampered = dir.join("tampered.exe");
        std::fs::write(&tampered, bytes).unwrap();
        assert!(!super::signed_by_vb_audio(tampered.to_str().unwrap()));
    }
}
