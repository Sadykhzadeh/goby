//! Updates: a signed manifest on the release pages of GitHub and got.az names the newest
//! version and the SHA-256 of its executable. The manifest counts only if its ECDSA P-256
//! signature checks out against the key built into Goby, so neither host, nor anyone who
//! takes one over, can hand out a different program. Both hosts are asked and the newest
//! verified version wins, so one of them going down or falling behind changes nothing.

use windows::Win32::Foundation::STATUS_SUCCESS;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    BCRYPT_SHA256_ALG_HANDLE, BCryptCloseAlgorithmProvider, BCryptDestroyKey, BCryptHash, BCryptImportKeyPair,
    BCryptOpenAlgorithmProvider, BCryptVerifySignature,
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::files::{self, File};
use crate::settings::whole;

/// The public half of the release key, X then Y. The private half is kept encrypted with
/// the author's other secrets and signs every release's manifest.
const PUBLIC_KEY: [u8; 64] = [
    0xa0, 0xf9, 0x7d, 0xff, 0xfd, 0x39, 0x3a, 0x5c, 0x76, 0x8c, 0xc4, 0x52, 0xa9, 0x0b, 0x7c, 0x1b, 0x03, 0x12, 0x8c,
    0x01, 0x75, 0x54, 0x9e, 0xff, 0x34, 0x65, 0xbb, 0xab, 0x9b, 0x72, 0x9e, 0x7a, 0x4c, 0xfa, 0x57, 0x05, 0x28, 0xd6,
    0x06, 0x94, 0xcc, 0x55, 0x2c, 0xb6, 0x1c, 0xb0, 0xef, 0xdc, 0x2b, 0x6d, 0x1a, 0x68, 0xe0, 0x17, 0x69, 0x50, 0x4f,
    0x6e, 0x01, 0xce, 0x62, 0xd6, 0x76, 0xd6,
];

/// Where each host keeps the manifest of its latest release, and its signature next to it.
const SOURCES: [&str; 2] = [
    "https://github.com/Sadykhzadeh/goby/releases/latest/download/goby-update",
    "https://got.az/azer/goby/releases/download/latest/goby-update",
];

/// Larger than any real manifest or executable; a host sending more is not trusted with it.
const MAX_MANIFEST: usize = 4 * 1024;
const MAX_EXE: u64 = 32 * 1024 * 1024;

pub type Version = (u32, u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub version: Version,
    pub size: u64,
    pub sha256: [u8; 32],
    /// Where the executable can be downloaded, tried in order.
    pub urls: Vec<String>,
}

pub fn parse_version(s: &str) -> Option<Version> {
    let mut parts = s.trim().split('.').map(|p| whole(p).and_then(|n| u32::try_from(n).ok()));
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

pub fn current() -> Version {
    parse_version(env!("CARGO_PKG_VERSION")).unwrap_or_default()
}

pub fn show(v: Version) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

fn hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.as_bytes();
    if s.len() != 64 {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 32];
    for (i, pair) in s.chunks(2).enumerate() {
        out[i] = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(out)
}

/// The manifest, strictly: a header line, then `version`, `size`, `sha256` and one or more
/// `url` lines. Anything else, an http address or a size past the limit is refused whole.
pub fn parse(text: &str) -> Option<Manifest> {
    let mut lines = text.lines();
    if lines.next()? != "goby update" {
        return None;
    }
    let (mut version, mut size, mut sha256, mut urls) = (None, None, None, Vec::new());
    for line in lines.filter(|l| !l.is_empty()) {
        let (key, value) = line.split_once(' ')?;
        match key {
            "version" if version.is_none() => version = Some(parse_version(value)?),
            "size" if size.is_none() => size = Some(whole(value).filter(|s| (1..=MAX_EXE).contains(s))?),
            "sha256" if sha256.is_none() => sha256 = Some(hex32(value)?),
            "url" if value.starts_with("https://") && !value.contains(char::is_whitespace) => {
                urls.push(value.to_string())
            }
            _ => return None,
        }
    }
    (!urls.is_empty()).then_some(())?;
    Some(Manifest { version: version?, size: size?, sha256: sha256?, urls })
}

/// `None` when Windows could not hash, so that nothing is ever compared with a hash that
/// was not computed.
pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    let hashed = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    (hashed == STATUS_SUCCESS).then_some(out)
}

/// Whether `signature` (r then s, 32 bytes each) signs `message` under `key` (X then Y).
fn verify_with(key: &[u8; 64], message: &[u8], signature: &[u8]) -> bool {
    if signature.len() != 64 {
        return false;
    }
    let header = BCRYPT_ECCKEY_BLOB { dwMagic: BCRYPT_ECDSA_PUBLIC_P256_MAGIC, cbKey: 32 };
    let mut blob = Vec::with_capacity(8 + 64);
    blob.extend(header.dwMagic.to_le_bytes());
    blob.extend(header.cbKey.to_le_bytes());
    blob.extend(key);
    let Some(hash) = sha256(message) else { return false };
    unsafe {
        let mut alg = BCRYPT_ALG_HANDLE::default();
        let opened = BCryptOpenAlgorithmProvider(
            &mut alg,
            BCRYPT_ECDSA_P256_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        );
        if opened != STATUS_SUCCESS {
            return false;
        }
        let mut handle = BCRYPT_KEY_HANDLE::default();
        let ok = BCryptImportKeyPair(alg, None, BCRYPT_ECCPUBLIC_BLOB, &mut handle, &blob, 0) == STATUS_SUCCESS
            && BCryptVerifySignature(handle, None, &hash, signature, BCRYPT_FLAGS(0)) == STATUS_SUCCESS;
        if !handle.is_invalid() {
            let _ = BCryptDestroyKey(handle);
        }
        let _ = BCryptCloseAlgorithmProvider(alg, 0);
        ok
    }
}

pub fn verify(message: &[u8], signature: &[u8]) -> bool {
    verify_with(&PUBLIC_KEY, message, signature)
}

/// GET over HTTPS through WinHTTP, which checks the certificate against Windows' store and
/// follows redirects only to HTTPS. `None` for anything but a 200 within `max` bytes.
pub fn get(url: &str, max: u64) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(h, p)| (h, format!("/{p}")));
    let agent = HSTRING::from(format!("Goby/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
        if session.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session, 10_000, 10_000, 10_000, 20_000);
        let body = (|| {
            let connect = WinHttpConnect(session, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0);
            if connect.is_null() {
                return None;
            }
            let request = WinHttpOpenRequest(
                connect,
                w!("GET"),
                &HSTRING::from(path.as_str()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            );
            let read = (|| {
                if request.is_null() {
                    return None;
                }
                WinHttpSendRequest(request, None, None, 0, 0, 0).ok()?;
                WinHttpReceiveResponse(request, std::ptr::null_mut()).ok()?;
                let mut status = 0u32;
                let mut len = 4u32;
                WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut status as *mut u32 as _),
                    &mut len,
                    std::ptr::null_mut(),
                )
                .ok()?;
                if status != 200 {
                    return None;
                }
                let mut out = Vec::new();
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let mut n = 0u32;
                    WinHttpReadData(request, buf.as_mut_ptr() as _, buf.len() as u32, &mut n).ok()?;
                    if n == 0 {
                        return Some(out);
                    }
                    out.extend_from_slice(&buf[..n as usize]);
                    if out.len() as u64 > max {
                        return None;
                    }
                }
            })();
            if !request.is_null() {
                let _ = WinHttpCloseHandle(request);
            }
            let _ = WinHttpCloseHandle(connect);
            read
        })();
        let _ = WinHttpCloseHandle(session);
        body
    }
}

/// The manifest from one host, if it is there and signed by the release key.
fn fetch(source: &str) -> Option<Manifest> {
    let text = get(&format!("{source}.txt"), MAX_MANIFEST as u64)?;
    let signature = get(&format!("{source}.sig"), 64)?;
    if !verify(&text, &signature) {
        return None;
    }
    parse(std::str::from_utf8(&text).ok()?)
}

/// The newest verified release on either host, when it is newer than this one. A host
/// offering an older version, as one replaying an old manifest would, is ignored.
pub fn check() -> Option<Manifest> {
    SOURCES.iter().filter_map(|s| fetch(s)).filter(|m| m.version > current()).max_by_key(|m| m.version)
}

/// Downloads the release, checks its size and SHA-256 against the signed manifest, and
/// returns it opened read-only with writes shut out, so it cannot change between the
/// check and its start. The handle is closed once the new copy has started.
pub fn download(m: &Manifest) -> Option<(String, File)> {
    let dir = format!("{}\\update", crate::settings::Settings::dir()?);
    files::create_dirs(&dir).ok()?;
    let path = format!("{dir}\\goby-{}.exe", show(m.version));
    let body = m.urls.iter().find_map(|url| get(url, m.size).filter(|b| b.len() as u64 == m.size))?;
    if sha256(&body) != Some(m.sha256) || !files::write(&path, &body) {
        return None;
    }
    let file = File::locked(&path)?;
    let again = file.read_all()?;
    (sha256(&again) == Some(m.sha256)).then_some((path, file))
}

/// Leftover downloads of earlier updates.
pub fn tidy() {
    if let Some(dir) = crate::settings::Settings::dir() {
        files::remove_dir_all(&format!("{dir}\\update"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986";

    fn manifest(extra: &str) -> String {
        format!("goby update\nversion 1.2.3\nsize 1000\nsha256 {SHA}\nurl https://example.org/a.exe\n{extra}")
    }

    #[test]
    fn a_well_formed_manifest_parses() {
        let m = parse(&manifest("url https://example.net/b.exe\n")).unwrap();
        assert_eq!(m.version, (1, 2, 3));
        assert_eq!(m.size, 1000);
        assert_eq!(m.sha256[0], 0x39);
        assert_eq!(m.urls, ["https://example.org/a.exe", "https://example.net/b.exe"]);
    }

    #[test]
    fn anything_unexpected_refuses_the_whole_manifest() {
        assert!(parse(&manifest("url http://example.org/plain.exe\n")).is_none());
        assert!(parse(&manifest("run calc.exe\n")).is_none());
        assert!(parse(&manifest("version 9.9.9\n")).is_none());
        assert!(parse(&manifest("").replace("goby update", "goby")).is_none());
        assert!(parse(&manifest("").replace("size 1000", "size 999999999")).is_none());
        assert!(parse(&manifest("").replace(SHA, "abc")).is_none());
        assert!(parse("goby update\nversion 1.2.3\nsize 10\nsha256 00\n").is_none());
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(parse_version("0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.x.3"), None);
        assert_eq!(show((0, 3, 0)), "0.3.0");
    }

    #[test]
    fn sha256_matches_the_standard_vector() {
        let expected = hex32("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap();
        assert_eq!(sha256(b"abc"), Some(expected));
    }

    /// `cargo test --release live_release -- --ignored --nocapture`: both hosts serve a
    /// manifest of the latest release signed by the release key, and the executable each
    /// one names downloads with the size and SHA-256 the manifest promises.
    #[test]
    #[ignore = "network"]
    fn live_release() {
        for source in SOURCES {
            let m = fetch(source).unwrap_or_else(|| panic!("no signed manifest at {source}"));
            let (path, _lock) = download(&m).expect("download that matches the manifest");
            println!("{source}: {} ({} bytes) -> {path}", show(m.version), m.size);
        }
    }

    /// A throwaway key, a manifest and its signature, made with OpenSSL the way releases
    /// are signed: `openssl dgst -sha256 -sign`, the DER signature rewritten as r then s.
    #[test]
    fn a_signature_must_come_from_the_key_and_cover_every_byte() {
        let key = include_bytes!("../tests/update/key.bin");
        let message = include_bytes!("../tests/update/goby-update.txt");
        let signature = include_bytes!("../tests/update/goby-update.sig");
        assert!(verify_with(key, message, signature));
        let mut changed = message.to_vec();
        changed[20] ^= 1;
        assert!(!verify_with(key, &changed, signature));
        let mut forged = signature.to_vec();
        forged[10] ^= 1;
        assert!(!verify_with(key, message, &forged));
        assert!(!verify_with(key, message, &signature[..63]));
        // The test key is not the release key: Goby itself would refuse this manifest.
        assert!(!verify(message, signature));
    }
}
