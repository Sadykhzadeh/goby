//! Builds the Windows resources into a .res file the MSVC linker takes directly: the icon,
//! the version details Explorer shows (product, author, copyright) and the manifest.
//! Written by hand so the build needs no resource compiler and no extra crates.

#[path = "src/icon.rs"]
mod icon;

use std::path::PathBuf;

const AUTHOR: &str = "Azer Sadykhzadeh";
/// The icon colour in Explorer, the taskbar pin and Settings > Apps.
const ICON_RGB: u32 = 0xFFB900;
const ICON_SIZES: [usize; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const RT_MANIFEST: u16 = 24;

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect()
}

fn pad4(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

/// One entry of a .res file: a 32-byte header with numeric type and id, then the data.
fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(32u32.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(kind.to_le_bytes());
    out.extend([0xFF, 0xFF]);
    out.extend(id.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // data version
    out.extend(0x1030u16.to_le_bytes()); // moveable | pure | discardable
    out.extend(0x0409u16.to_le_bytes()); // en-US
    out.extend(0u32.to_le_bytes()); // version
    out.extend(0u32.to_le_bytes()); // characteristics
    out.extend(data);
    pad4(out);
}

/// A 32-bit icon image: BITMAPINFOHEADER, bottom-up BGRA, then an all-clear AND mask.
fn icon_image(size: usize) -> Vec<u8> {
    let mut b = Vec::new();
    let mask_row = size.div_ceil(32) * 4;
    for v in [40u32, size as u32, size as u32 * 2] {
        b.extend(v.to_le_bytes());
    }
    b.extend(1u16.to_le_bytes());
    b.extend(32u16.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(((size * size * 4 + mask_row * size) as u32).to_le_bytes());
    b.extend([0u8; 16]);
    let px = icon::pixels(size, ICON_RGB);
    for row in (0..size).rev() {
        for p in &px[row * size..(row + 1) * size] {
            b.extend(p.to_le_bytes());
        }
    }
    b.extend(std::iter::repeat_n(0u8, mask_row * size));
    b
}

/// One VS_VERSIONINFO node: header, key, value, children, each 32-bit aligned.
fn node(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b.extend(utf16z(key));
    pad4(&mut b);
    b.extend(value);
    pad4(&mut b);
    for c in children {
        b.extend(c);
        pad4(&mut b);
    }
    let value_len = if text { value.len() / 2 } else { value.len() };
    let total = b.len() as u16;
    b[0..2].copy_from_slice(&total.to_le_bytes());
    b[2..4].copy_from_slice(&(value_len as u16).to_le_bytes());
    b[4..6].copy_from_slice(&(text as u16).to_le_bytes());
    b
}

fn version_info(version: &str) -> Vec<u8> {
    let parts: Vec<u32> = version.split('.').map(|p| p.parse().unwrap_or(0)).chain([0, 0, 0]).take(3).collect();
    let (ms, ls) = (parts[0] << 16 | parts[1], parts[2] << 16);
    let fixed: Vec<u8> = [0xFEEF04BD, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, 1, 0, 0, 0]
        .iter()
        .flat_map(|v: &u32| v.to_le_bytes())
        .collect();
    let year = "2026";
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", AUTHOR.to_string()),
        ("FileDescription", "Goby voice changer".to_string()),
        ("FileVersion", version.to_string()),
        ("InternalName", "goby".to_string()),
        ("LegalCopyright", format!("© {year} {AUTHOR}")),
        ("OriginalFilename", "goby.exe".to_string()),
        ("ProductName", "Goby".to_string()),
        ("ProductVersion", version.to_string()),
        ("Comments", "https://azer.one".to_string()),
    ]
    .iter()
    .map(|(k, v)| node(k, &utf16z(v), true, &[]))
    .collect();
    let table = node("040904B0", &[], true, &strings);
    let string_info = node("StringFileInfo", &[], true, &[table]);
    let translation = node("Translation", &[0x09, 0x04, 0xB0, 0x04], false, &[]);
    let var_info = node("VarFileInfo", &[], true, &[translation]);
    node("VS_VERSION_INFO", &fixed, false, &[string_info, var_info])
}

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="AzerSadykhzadeh.Goby" version="1.0.0.0"/>
  <description>Goby voice changer</description>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    // A .res file starts with one empty resource.
    let mut res = vec![0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
    res.extend([0u8; 16]);

    let mut group = Vec::new();
    group.extend([0u16, 1, ICON_SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    for (i, size) in ICON_SIZES.iter().enumerate() {
        let image = icon_image(*size);
        let id = i as u16 + 1;
        resource(&mut res, RT_ICON, id, &image);
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        group.extend([dim, dim, 0, 0]);
        group.extend(1u16.to_le_bytes());
        group.extend(32u16.to_le_bytes());
        group.extend((image.len() as u32).to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    resource(&mut res, RT_GROUP_ICON, 1, &group);
    resource(&mut res, RT_VERSION, 1, &version_info(env!("CARGO_PKG_VERSION")));
    resource(&mut res, RT_MANIFEST, 1, MANIFEST.as_bytes());

    let path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("goby.res");
    std::fs::write(&path, res).expect("write goby.res");
    println!("cargo:rustc-link-arg-bins={}", path.display());
}
