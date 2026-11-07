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

fn len_symbol(len: usize) -> usize {
    LEN_BASE.iter().rposition(|&base| base as usize <= len).unwrap()
}

fn dist_symbol(dist: usize) -> usize {
    DIST_BASE.iter().rposition(|&base| base as usize <= dist).unwrap()
}

/// Huffman code lengths for `freq`, none longer than `limit`. Unused symbols get 0. When
/// the tree comes out too deep, the counts are halved towards each other and it is redone.
fn code_lengths(freq: &[u64], limit: u8) -> Vec<u8> {
    let mut freq = freq.to_vec();
    loop {
        let mut lens = vec![0u8; freq.len()];
        let used: Vec<usize> = (0..freq.len()).filter(|&s| freq[s] > 0).collect();
        if used.len() == 1 {
            lens[used[0]] = 1;
            return lens;
        }
        // Each node is (count, parent); leaves come first, in the order of `used`.
        let mut nodes: Vec<(u64, usize)> = used.iter().map(|&s| (freq[s], usize::MAX)).collect();
        let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(u64, usize)>> =
            nodes.iter().enumerate().map(|(i, n)| std::cmp::Reverse((n.0, i))).collect();
        while heap.len() > 1 {
            let std::cmp::Reverse((a, i)) = heap.pop().unwrap();
            let std::cmp::Reverse((b, j)) = heap.pop().unwrap();
            let parent = nodes.len();
            nodes.push((a + b, usize::MAX));
            nodes[i].1 = parent;
            nodes[j].1 = parent;
            heap.push(std::cmp::Reverse((a + b, parent)));
        }
        let mut deepest = 0;
        for (leaf, &s) in used.iter().enumerate() {
            let (mut depth, mut at) = (0u8, leaf);
            while nodes[at].1 != usize::MAX {
                at = nodes[at].1;
                depth += 1;
            }
            lens[s] = depth;
            deepest = deepest.max(depth);
        }
        if deepest <= limit {
            return lens;
        }
        for f in freq.iter_mut().filter(|f| **f > 0) {
            *f = f.div_ceil(2);
        }
    }
}

/// Canonical codes for code lengths, as RFC 1951 assigns them.
fn canonical(lens: &[u8]) -> Vec<u32> {
    let mut next = [0u32; 16];
    let mut count = [0u32; 16];
    lens.iter().filter(|&&l| l > 0).for_each(|&l| count[l as usize] += 1);
    for bits in 1..16 {
        next[bits] = (next[bits - 1] + count[bits - 1]) << 1;
    }
    lens.iter()
        .map(|&l| {
            let code = next[l as usize];
            next[l as usize] += 1;
            code
        })
        .collect()
}

/// For every position, the shortest distance that gives each match length, as a list of
/// (longest length, distance) steps; found through hash chains over the whole window.
fn matches(data: &[u8]) -> Vec<Vec<(usize, usize)>> {
    const WINDOW: usize = 32768;
    let hash = |i: usize| (data[i] as usize) << 16 | (data[i + 1] as usize) << 8 | data[i + 2] as usize;
    let mut head = std::collections::HashMap::new();
    let mut prev = vec![usize::MAX; data.len()];
    let mut found = vec![Vec::new(); data.len()];
    for i in 0..data.len().saturating_sub(2) {
        let h = hash(i);
        let (mut at, mut longest, mut tries) = (*head.get(&h).unwrap_or(&usize::MAX), 2, 0);
        let most = (data.len() - i).min(258);
        // Build scripts run unoptimised, so a candidate that cannot beat the longest so far
        // is turned away on one byte.
        while at != usize::MAX && i - at <= WINDOW && tries < 256 && longest < most {
            if data[at + longest] == data[i + longest] {
                let mut n = 0;
                while n < most && data[at + n] == data[i + n] {
                    n += 1;
                }
                if n > longest {
                    longest = n;
                    found[i].push((n, i - at));
                }
            }
            at = prev[at];
            tries += 1;
        }
        prev[i] = head.insert(h, i).unwrap_or(usize::MAX);
    }
    found
}

/// Literals and (length, distance) matches; a literal has length 0 and the byte as distance.
type Tokens = Vec<(usize, usize)>;

/// The cheapest way through `data` under the given code lengths, by shortest path.
fn parse(data: &[u8], found: &[Vec<(usize, usize)>], lit: &[u8], dist: &[u8]) -> Tokens {
    let cost_of = |lens: &[u8], s: usize| if lens[s] > 0 { lens[s] as u32 } else { 16 };
    let lit_cost: Vec<u32> = (0..256).map(|s| cost_of(lit, s)).collect();
    let len_cost: Vec<u32> = (0..259)
        .map(|len| if len < 3 { 0 } else { cost_of(lit, 257 + len_symbol(len)) + LEN_EXTRA[len_symbol(len)] })
        .collect();
    let mut best = vec![(u32::MAX, (0usize, 0usize)); data.len() + 1];
    best[0].0 = 0;
    for i in 0..data.len() {
        let here = best[i].0;
        let c = here + lit_cost[data[i] as usize];
        if c < best[i + 1].0 {
            best[i + 1] = (c, (0, data[i] as usize));
        }
        let mut from = 3;
        for &(upto, d) in &found[i] {
            let ds = dist_symbol(d);
            let base = here + cost_of(dist, ds) + DIST_EXTRA[ds];
            for len in from..=upto {
                let c = base + len_cost[len];
                if c < best[i + len].0 {
                    best[i + len] = (c, (len, d));
                }
            }
            from = upto + 1;
        }
    }
    let mut tokens = Vec::new();
    let mut i = data.len();
    while i > 0 {
        let t = best[i].1;
        tokens.push(t);
        i -= t.0.max(1);
    }
    tokens.reverse();
    tokens
}

/// How often each literal/length and distance symbol occurs.
fn counts(tokens: &Tokens) -> (Vec<u64>, Vec<u64>) {
    let (mut lit, mut dist) = (vec![0u64; 286], vec![0u64; 30]);
    for &(len, d) in tokens {
        if len == 0 {
            lit[d] += 1;
        } else {
            lit[257 + len_symbol(len)] += 1;
            dist[dist_symbol(d)] += 1;
        }
    }
    lit[256] = 1;
    // Some decoders want at least two distance codes.
    for d in &mut dist[..2] {
        *d = (*d).max(1);
    }
    (lit, dist)
}

/// One dynamic-Huffman deflate block. The parse is refined a few times, each round costed
/// with the codes the round before produced, much as zopfli does.
fn deflate(data: &[u8]) -> Vec<u8> {
    let found = matches(data);
    let (mut lit, mut dist) = (vec![8u8; 286], vec![5u8; 30]);
    let mut best: Option<Vec<u8>> = None;
    for _ in 0..4 {
        let tokens = parse(data, &found, &lit, &dist);
        let (lf, df) = counts(&tokens);
        (lit, dist) = (code_lengths(&lf, 15), code_lengths(&df, 15));
        let out = write_block(&tokens, &lit, &dist);
        if best.as_ref().is_none_or(|b| out.len() < b.len()) {
            best = Some(out);
        }
    }
    best.unwrap()
}

fn write_block(tokens: &Tokens, lit: &[u8], dist: &[u8]) -> Vec<u8> {
    let hlit = 257.max(lit.iter().rposition(|&l| l > 0).unwrap() + 1);
    let hdist = 1.max(dist.iter().rposition(|&l| l > 0).unwrap_or(0) + 1);
    let all: Vec<u8> = lit[..hlit].iter().chain(&dist[..hdist]).copied().collect();
    // Run-length the code lengths: 16 repeats the last 3-6 times, 17 and 18 are zero runs.
    let mut rle: Vec<(usize, u32)> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let run = all[i..].iter().take_while(|&&l| l == all[i]).count();
        if all[i] == 0 && run >= 11 {
            let n = run.min(138);
            rle.push((18, n as u32 - 11));
            i += n;
        } else if all[i] == 0 && run >= 3 {
            rle.push((17, run as u32 - 3));
            i += run;
        } else if i > 0 && all[i] == all[i - 1] && run >= 3 {
            let n = run.min(6);
            rle.push((16, n as u32 - 3));
            i += n;
        } else {
            rle.push((all[i] as usize, 0));
            i += 1;
        }
    }
    let mut cf = vec![0u64; 19];
    rle.iter().for_each(|&(s, _)| cf[s] += 1);
    let cl = code_lengths(&cf, 7);
    let cc = canonical(&cl);
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let hclen = 4.max(ORDER.iter().rposition(|&s| cl[s] > 0).unwrap() + 1);

    let mut b = Bits { out: Vec::new(), acc: 0, n: 0 };
    b.put(1, 1); // last block
    b.put(2, 2); // dynamic Huffman codes
    b.put(hlit as u32 - 257, 5);
    b.put(hdist as u32 - 1, 5);
    b.put(hclen as u32 - 4, 4);
    for &s in &ORDER[..hclen] {
        b.put(cl[s] as u32, 3);
    }
    for &(s, extra) in &rle {
        b.code(cc[s], cl[s] as u32);
        match s {
            16 => b.put(extra, 2),
            17 => b.put(extra, 3),
            18 => b.put(extra, 7),
            _ => {}
        }
    }
    let (lc, dc) = (canonical(lit), canonical(dist));
    for &(len, d) in tokens {
        if len == 0 {
            b.code(lc[d], lit[d] as u32);
        } else {
            let k = len_symbol(len);
            b.code(lc[257 + k], lit[257 + k] as u32);
            b.put((len as u32) - LEN_BASE[k], LEN_EXTRA[k]);
            let k = dist_symbol(d);
            b.code(dc[k], dist[k] as u32);
            b.put(d as u32 - DIST_BASE[k], DIST_EXTRA[k]);
        }
    }
    b.code(lc[256], lit[256] as u32);
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

/// The PNG filter `kind` applied to one row, given the row above (all zero for the first)
/// and the bytes a pixel takes.
fn filter(kind: u8, row: &[u8], up: &[u8], bpp: usize) -> Vec<u8> {
    let mut out = vec![kind];
    for i in 0..row.len() {
        let (a, b, c) = if i >= bpp { (row[i - bpp], up[i], up[i - bpp]) } else { (0, up[i], 0) };
        let predicted = match kind {
            0 => 0,
            1 => a,
            2 => b,
            3 => ((a as u16 + b as u16) / 2) as u8,
            _ => {
                let p = a as i16 + b as i16 - c as i16;
                let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
                if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                }
            }
        };
        out.push(row[i].wrapping_sub(predicted));
    }
    out
}

/// Image data for a PNG: the rows filtered with whichever of kinds packs smallest.
fn idat(rows: &[Vec<u8>], bpp: usize, kinds: std::ops::Range<u8>) -> Vec<u8> {
    let zero = vec![0; rows[0].len()];
    kinds
        .map(|kind| {
            let mut raw = Vec::new();
            for (y, row) in rows.iter().enumerate() {
                raw.extend(filter(kind, row, if y > 0 { &rows[y - 1] } else { &zero }, bpp));
            }
            let mut zlib = vec![0x78, 0x01];
            zlib.extend(deflate(&raw));
            zlib.extend(adler32(&raw).to_be_bytes());
            zlib
        })
        .min_by_key(Vec::len)
        .unwrap()
}

fn png(size: usize, depth: u8, colour: u8, chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut header = Vec::new();
    header.extend((size as u32).to_be_bytes());
    header.extend((size as u32).to_be_bytes());
    header.extend([depth, colour, 0, 0, 0]);
    for (kind, data) in [(b"IHDR", header)].iter().chain(chunks).chain(&[(b"IEND", Vec::new())]) {
        png.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        png.extend(&body);
        png.extend(crc32(&body).to_be_bytes());
    }
    png
}

/// A PNG of the icon. Windows has read PNG images inside icons since Vista, at every size.
/// The icon is one colour at many opacities, so besides plain RGBA it is tried as a palette
/// of those opacities at a byte or less a pixel; both decode to the RGBA icon.rs draws, and
/// the smaller is kept. The palette's own size makes RGBA win at the small sizes.
fn icon_png(size: usize) -> Vec<u8> {
    let px = icon::pixels(size, icon::RGB);
    let rgba: Vec<Vec<u8>> = px
        .chunks(size)
        .map(|row| row.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]).collect())
        .collect();
    // Filters never help the RGBA form here, and trying them makes the build slow.
    let mut best = png(size, 8, 6, &[(b"IDAT", idat(&rgba, 4, 0..1))]);

    let mut palette: Vec<u32> = px.clone();
    palette.sort_by_key(|p| p >> 24);
    palette.dedup();
    let index: Vec<u8> =
        px.iter().map(|p| palette.binary_search_by_key(&(p >> 24), |q| q >> 24).unwrap() as u8).collect();
    let colours: Vec<u8> = palette.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
    // Opacity per palette entry; the last, fully opaque one may be left out.
    let alpha: Vec<u8> = palette.iter().map(|p| (p >> 24) as u8).collect();
    let alpha = alpha.strip_suffix(&[255]).unwrap_or(&alpha).to_vec();
    for depth in [1u8, 2, 4, 8].into_iter().filter(|d| palette.len() <= 1 << d) {
        let per = 8 / depth as usize;
        let rows: Vec<Vec<u8>> = index
            .chunks(size)
            .map(|row| {
                row.chunks(per)
                    .map(|c| c.iter().enumerate().fold(0, |b, (k, &v)| b | v << (8 - depth as usize * (k + 1))))
                    .collect()
            })
            .collect();
        let candidate = png(
            size,
            depth,
            3,
            &[(b"PLTE", colours.clone()), (b"tRNS", alpha.clone()), (b"IDAT", idat(&rows, 1, 0..5))],
        );
        if candidate.len() < best.len() {
            best = candidate;
        }
    }
    best
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
        ("LegalCopyright", format!("Â© {year} {AUTHOR}, GPL-3.0-or-later")),
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

    // With crt-static (.cargo/config.toml) the VC runtime is linked in, and these two take
    // the Universal CRT from Windows (ucrtbase.dll, part of Windows 10 and 11) instead of
    // copying it into the exe: 95 KB less than a fully static CRT.
    println!("cargo:rustc-link-arg-bins=/NODEFAULTLIB:libucrt.lib");
    println!("cargo:rustc-link-arg-bins=/DEFAULTLIB:ucrt.lib");

    if std::env::var("PROFILE").as_deref() == Ok("release") {
        // strip = true still leaves rustc passing /DEBUG, so the exe carries a debug directory
        // naming a goby.pdb that no release keeps. Debug builds keep their PDB.
        println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");
        // Leaves out the POGO debug entry: a list of section names only profile-guided
        // optimization tools read.
        println!("cargo:rustc-link-arg-bins=/EMITPOGOPHASEINFO");
    }
}
