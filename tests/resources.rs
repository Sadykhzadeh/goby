//! Reads the icon and the version details back out of the built executable, the way
//! Explorer does, so a change to how build.rs packs them cannot go unnoticed.

#![cfg(target_env = "msvc")]

#[path = "../src/icon.rs"]
mod icon;

use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, ReleaseDC,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW};

const EXE: &str = env!("CARGO_BIN_EXE_goby");

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// The colour pixels of the icon Explorer would pick at `size`, as straight BGRA.
fn extract(size: usize) -> Vec<u32> {
    let mut path = [0u16; 260];
    let w = wide(EXE);
    path[..w.len()].copy_from_slice(&w);
    let mut icon = [HICON::default()];
    unsafe {
        let n = PrivateExtractIconsW(&path, 0, size as i32, size as i32, Some(&mut icon), None, 0);
        assert_eq!(n, 1, "no {size}px icon");
        let mut info = ICONINFO::default();
        GetIconInfo(icon[0], &mut info).unwrap();
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u32; size * size];
        let dc = GetDC(None);
        let rows = GetDIBits(dc, info.hbmColor, 0, size as u32, Some(px.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        assert_eq!(rows, size as i32);
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        DestroyIcon(icon[0]).unwrap();
        px
    }
}

#[test]
fn every_icon_size_reads_back_exactly_as_drawn() {
    for size in [16, 20, 24, 32, 40, 48, 64, 256] {
        assert!(extract(size) == icon::pixels(size, icon::RGB), "the {size}px icon differs from icon.rs");
    }
}

#[link(name = "version")]
unsafe extern "system" {
    fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(file: *const u16, handle: u32, len: u32, data: *mut u8) -> i32;
    fn VerQueryValueW(block: *const u8, sub: *const u16, buffer: *mut *const u16, len: *mut u32) -> i32;
}

#[test]
fn version_details_read_back() {
    let file = wide(EXE);
    let mut data = vec![0u8; unsafe { GetFileVersionInfoSizeW(file.as_ptr(), &mut 0) } as usize];
    assert!(!data.is_empty());
    assert!(unsafe { GetFileVersionInfoW(file.as_ptr(), 0, data.len() as u32, data.as_mut_ptr()) } != 0);
    let query = |key: &str| {
        let (mut p, mut len) = (std::ptr::null(), 0);
        let sub = wide(&format!("\\StringFileInfo\\040904B0\\{key}"));
        assert!(unsafe { VerQueryValueW(data.as_ptr(), sub.as_ptr(), &mut p, &mut len) } != 0, "{key}");
        let text = unsafe { std::slice::from_raw_parts(p, len as usize) };
        String::from_utf16_lossy(text).trim_end_matches('\0').to_string()
    };
    assert_eq!(query("ProductName"), "Goby");
    assert_eq!(query("FileDescription"), "Goby voice changer");
    assert_eq!(query("CompanyName"), "Azer Sadykhzadeh");
    assert_eq!(query("FileVersion"), env!("CARGO_PKG_VERSION"));
    assert_eq!(query("OriginalFilename"), "goby.exe");
    assert!(query("ProductVersion").starts_with(env!("CARGO_PKG_VERSION")));
    assert!(query("LegalCopyright").contains("GPL-3.0-or-later"));
}
