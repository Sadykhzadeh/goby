//! Builds the Windows resources into a .res file the MSVC linker takes directly: the icon,
//! the version details Explorer shows (product, author, copyright) and the manifest.
//! Written by hand so the build needs no resource compiler and no extra crates.

#[path = "src/icon.rs"]
mod icon;

use std::path::PathBuf;

const AUTHOR: &str = "Azer Sadykhzadeh";
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

/// Writes deflate bits: values least-significant bit first, Huffman codes most-significant first.
struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        self.acc |= value << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn code(&mut self, code: u32, len: u32) {
        let reversed = (0..len).fold(0, |r, i| r | ((code >> i) & 1) << (len - 1 - i));
        self.put(reversed, len);
    }

    /// A literal or length symbol in the fixed Huffman code of RFC 1951.
    fn symbol(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xC0 + v - 280, 8),
        }
    }
}

const LEN_BASE: [u32; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// One fixed-Huffman deflate block whose matches look one pixel or one row back. Icons are
/// mostly transparent runs, so this small encoder already shrinks them about tenfold.
fn deflate(data: &[u8], row: usize) -> Vec<u8> {
    let mut b = Bits { out: Vec::new(), acc: 0, n: 0 };
    b.put(1, 1); // last block
    b.put(1, 2); // fixed Huffman codes
    let mut i = 0;
    while i < data.len() {
        let mut best = (0usize, 0usize);
        for d in [4, row] {
            if d <= i {
                let n = (0..258).take_while(|&k| i + k < data.len() && data[i + k] == data[i + k - d]).count();
                if n > best.0 {
                    best = (n, d);
                }
            }
        }
        if best.0 >= 3 {
            let (len, dist) = (best.0 as u32, best.1 as u32);
            let k = LEN_BASE.iter().rposition(|&base| base <= len).unwrap();
            b.symbol(257 + k as u32);
            b.put(len - LEN_BASE[k], LEN_EXTRA[k]);
            let k = DIST_BASE.iter().rposition(|&base| base <= dist).unwrap();
            b.code(k as u32, 5);
            b.put(dist - DIST_BASE[k], DIST_EXTRA[k]);
            i += best.0;
        } else {
            b.symbol(data[i] as u32);
            i += 1;
        }
    }
    b.symbol(256);
    if b.n > 0 {
        b.out.push(b.acc as u8);
    }
    b.out
}

fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        (0..8).fold(crc ^ byte as u32, |c, _| if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 })
    })
}

fn adler32(bytes: &[u8]) -> u32 {
    let (a, b) = bytes.iter().fold((1u32, 0u32), |(a, b), &x| {
        let a = (a + x as u32) % 65521;
        (a, (b + a) % 65521)
    });
    b << 16 | a
}

/// An RGBA PNG of the icon. Windows has read PNG images inside icons since Vista, at
/// every size, and they take a tenth of the room of 32-bit bitmaps.
fn icon_png(size: usize) -> Vec<u8> {
    let px = icon::pixels(size, icon::RGB);
    let mut raw = Vec::with_capacity(size * (1 + size * 4));
    for row in px.chunks(size) {
        raw.push(0); // no filter
        for p in row {
            raw.extend([(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
        }
    }
    let mut zlib = vec![0x78, 0x01];
    zlib.extend(deflate(&raw, 1 + size * 4));
    zlib.extend(adler32(&raw).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        png.extend(&body);
        png.extend(crc32(&body).to_be_bytes());
    };
    let mut header = Vec::new();
    header.extend((size as u32).to_be_bytes());
    header.extend((size as u32).to_be_bytes());
    header.extend([8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    png
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

fn version_info(version: &str, commit: &str) -> Vec<u8> {
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
        ("LegalCopyright", format!("© {year} {AUTHOR}, GPL-3.0-or-later")),
        ("OriginalFilename", "goby.exe".to_string()),
        ("ProductName", "Goby".to_string()),
        // The commit rides along here, so Properties > Details names the exact build.
        ("ProductVersion", if commit.is_empty() { version.to_string() } else { format!("{version} ({commit})") }),
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

/// The commit this build comes from, with "-modified" when the tracked files differ from
/// it; empty outside a git checkout (a source tarball, say).
fn commit() -> String {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let Some(head) = git(&["rev-parse", "--short=7", "HEAD"]) else { return String::new() };
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    if dirty { format!("{head}-modified") } else { head }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/refs");
    let commit = commit();
    println!("cargo:rustc-env=GOBY_COMMIT={commit}");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }

    // A .res file starts with one empty resource.
    let mut res = vec![0, 0, 0, 0, 32, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0];
    res.extend([0u8; 16]);

    let mut group = Vec::new();
    group.extend([0u16, 1, ICON_SIZES.len() as u16].iter().flat_map(|v| v.to_le_bytes()));
    for (i, size) in ICON_SIZES.iter().enumerate() {
        let image = icon_png(*size);
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
    resource(&mut res, RT_VERSION, 1, &version_info(env!("CARGO_PKG_VERSION"), &commit));
    resource(&mut res, RT_MANIFEST, 1, MANIFEST.as_bytes());

    let path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("goby.res");
    std::fs::write(&path, res).expect("write goby.res");
    println!("cargo:rustc-link-arg-bins={}", path.display());

    if std::env::var("PROFILE").as_deref() == Ok("release") {
        // strip = true still leaves rustc passing /DEBUG, so the exe carries a debug directory
        // naming a goby.pdb that no release keeps. Debug builds keep their PDB.
        println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");
        // Leaves out the POGO debug entry: a list of section names only profile-guided
        // optimization tools read.
        println!("cargo:rustc-link-arg-bins=/EMITPOGOPHASEINFO");
    }
}
