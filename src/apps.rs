//! Which programs use the microphone. Windows keeps this list itself for the privacy page
//! (Settings > Privacy > Microphone > recent activity); we read the same registry keys.

use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, ReleaseDC,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, PrivateExtractIconsW};
use windows::core::{PWSTR, w};

#[derive(Clone, Debug, PartialEq)]
pub struct MicApp {
    /// Lower-case executable name, e.g. "cs2.exe": stable across updates that move the folder.
    pub exe: String,
    /// What to show: the executable name without ".exe".
    pub name: String,
    /// Where the executable lives, for its icon.
    pub path: String,
    /// FILETIME of the last time it opened the mic.
    pub last_used: u64,
    /// It has the mic open right now.
    pub active: bool,
}

/// Helpers and system processes that use the mic on someone else's behalf.
const IGNORED: [&str; 4] = ["rundll32.exe", "svchost.exe", "goby.exe", "audiodg.exe"];

/// The registry stores the path with `#` for every `\`.
pub fn exe_of(key: &str) -> Option<String> {
    let file = key.rsplit('#').next()?.to_ascii_lowercase();
    file.ends_with(".exe").then_some(file)
}

/// What people call a program: the product name for well-known ones, whose executables
/// are often cryptic (`cs2.exe`, `obs64.exe`), otherwise the file name with a capital.
pub fn display_name(exe: &str) -> String {
    let known = match exe {
        "cs2.exe" => "Counter-Strike 2",
        "dota2.exe" => "Dota 2",
        "r5apex.exe" | "r5apex_dx12.exe" => "Apex Legends",
        "valorant-win64-shipping.exe" => "Valorant",
        "fortniteclient-win64-shipping.exe" => "Fortnite",
        "robloxplayerbeta.exe" => "Roblox",
        "javaw.exe" => "Minecraft",
        "discord.exe" => "Discord",
        "telegram.exe" => "Telegram",
        "teams.exe" | "ms-teams.exe" => "Microsoft Teams",
        "zoom.exe" => "Zoom",
        "skype.exe" => "Skype",
        "whatsapp.exe" => "WhatsApp",
        "obs64.exe" | "obs32.exe" => "OBS Studio",
        "chrome.exe" => "Google Chrome",
        "msedge.exe" => "Microsoft Edge",
        "firefox.exe" => "Firefox",
        "steam.exe" | "steamwebhelper.exe" => "Steam",
        "nvidia broadcast.exe" => "NVIDIA Broadcast",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // ASCII only: full Unicode case mapping would add 17 KB of tables for one letter.
    let mut name = exe.strip_suffix(".exe").unwrap_or(exe).to_string();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    name
}

/// Newest entry per executable, most recently used first, without system helpers.
pub fn dedupe(mut found: Vec<MicApp>) -> Vec<MicApp> {
    found.retain(|a| !IGNORED.contains(&a.exe.as_str()));
    found.sort_by(|a, b| b.active.cmp(&a.active).then(b.last_used.cmp(&a.last_used)));
    let mut seen = Vec::<String>::new();
    found.retain(|a| {
        let new = !seen.contains(&a.exe);
        seen.push(a.exe.clone());
        new
    });
    found
}

/// Programs that have used the microphone, most recent first.
pub fn recent() -> Vec<MicApp> {
    let mut found = Vec::new();
    unsafe {
        let mut root = HKEY::default();
        let path = w!(
            r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged"
        );
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, None, KEY_READ, &mut root) != ERROR_SUCCESS {
            return found;
        }
        let mut name = [0u16; 1024];
        for index in 0.. {
            let mut len = name.len() as u32;
            let r = RegEnumKeyExW(root, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
            if r == ERROR_NO_MORE_ITEMS || r != ERROR_SUCCESS {
                break;
            }
            let key = String::from_utf16_lossy(&name[..len as usize]);
            let Some(exe) = exe_of(&key) else { continue };
            let read = |value: windows::core::PCWSTR| {
                let (mut v, mut size) = (0u64, 8u32);
                let ok = RegGetValueW(
                    root,
                    windows::core::PCWSTR(name.as_ptr()),
                    value,
                    RRF_RT_REG_QWORD,
                    None,
                    Some(&mut v as *mut u64 as _),
                    Some(&mut size),
                ) == ERROR_SUCCESS;
                ok.then_some(v)
            };
            let start = read(w!("LastUsedTimeStart")).unwrap_or(0);
            let stop = read(w!("LastUsedTimeStop")).unwrap_or(1);
            found.push(MicApp {
                name: display_name(&exe),
                path: key.replace('#', "\\"),
                exe,
                last_used: start,
                active: start > 0 && stop == 0,
            });
        }
        let _ = RegCloseKey(root);
    }
    dedupe(found)
}

/// The head of one `SYSTEM_PROCESS_INFORMATION` entry, as winternl.h declares it.
#[repr(C)]
struct ProcessEntry {
    next: u32,
    threads: u32,
    reserved: [u8; 48],
    name_bytes: u16,
    name_capacity: u16,
    name: *const u16,
    base_priority: i32,
    pid: usize,
}

#[link(name = "ntdll", kind = "raw-dylib")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut std::ffi::c_void, len: u32, returned: *mut u32) -> i32;
}

/// Every running process as (lower-case exe name, pid), from one snapshot.
///
/// This asks the kernel directly, as Toolhelp does underneath: Toolhelp copies the same
/// answer into a section and back out one entry at a time, which made it take 8 ms
/// instead of 3 on a machine running 350 processes.
pub fn processes() -> Vec<(String, u32)> {
    const SYSTEM_PROCESS_INFORMATION: u32 = 5;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    let mut all = Vec::new();
    // u64s keep the entries aligned. Left uninitialised: the kernel fills what it reports.
    let mut buf: Vec<u64> = Vec::with_capacity(128 * 1024);
    loop {
        let mut needed = 0u32;
        let bytes = (buf.capacity() * 8).min(u32::MAX as usize) as u32;
        let status =
            unsafe { NtQuerySystemInformation(SYSTEM_PROCESS_INFORMATION, buf.as_mut_ptr() as _, bytes, &mut needed) };
        match status {
            0 => break,
            // Processes start between the two calls, so ask for some room to spare.
            STATUS_INFO_LENGTH_MISMATCH if needed as usize > bytes as usize => {
                buf = Vec::with_capacity(needed as usize / 8 + 8192);
            }
            STATUS_INFO_LENGTH_MISMATCH => buf = Vec::with_capacity(buf.capacity() * 2),
            _ => return all,
        }
    }
    let base = buf.as_ptr() as *const u8;
    let mut at = 0usize;
    loop {
        let entry = unsafe { &*(base.add(at) as *const ProcessEntry) };
        let name = match entry.name.is_null() {
            true => String::new(),
            false => {
                let wide = unsafe { std::slice::from_raw_parts(entry.name, entry.name_bytes as usize / 2) };
                String::from_utf16_lossy(wide).to_ascii_lowercase()
            }
        };
        all.push((name, entry.pid as u32));
        if entry.next == 0 {
            break;
        }
        at += entry.next as usize;
    }
    all
}

/// Process ids of `exe` (lower-case file name) in a `processes()` snapshot.
pub fn pids_of(snapshot: &[(String, u32)], exe: &str) -> Vec<u32> {
    snapshot.iter().filter(|(name, _)| name == exe).map(|(_, pid)| *pid).collect()
}

/// The full path of a running process's executable.
pub fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// A program's own icon at `size`×`size` pixels, as premultiplied BGRA rows, or `None`
/// when the file has no icon to give.
pub fn icon_pixels(path: &str, size: i32) -> Option<Vec<u8>> {
    unsafe {
        let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
        let mut name = [0u16; 260];
        let n = wide.len().min(name.len());
        name[..n].copy_from_slice(&wide[..n]);
        let mut icon = HICON::default();
        if PrivateExtractIconsW(&name, 0, size, size, Some(std::slice::from_mut(&mut icon)), None, 0) == 0
            || icon.is_invalid()
        {
            return None;
        }
        let mut info = ICONINFO::default();
        let got = GetIconInfo(icon, &mut info).is_ok();
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = GetDC(None);
        let rows = if got {
            GetDIBits(dc, info.hbmColor, 0, size as u32, Some(pixels.as_mut_ptr() as _), &mut header, DIB_RGB_COLORS)
        } else {
            0
        };
        // Old icons carry no alpha at all and use a separate mask for transparency.
        if rows > 0 && pixels.chunks_exact(4).all(|p| p[3] == 0) {
            let mut mask = vec![0u8; (size * size * 4) as usize];
            GetDIBits(dc, info.hbmMask, 0, size as u32, Some(mask.as_mut_ptr() as _), &mut header, DIB_RGB_COLORS);
            for (p, m) in pixels.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        ReleaseDC(None, dc);
        let _ = DeleteObject(info.hbmColor.into());
        let _ = DeleteObject(info.hbmMask.into());
        let _ = DestroyIcon(icon);
        if rows == 0 {
            return None;
        }
        for p in pixels.chunks_exact_mut(4) {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
        Some(pixels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    fn app(exe: &str, last_used: u64, active: bool) -> MicApp {
        MicApp { exe: exe.into(), name: display_name(exe), path: String::new(), last_used, active }
    }

    #[test]
    fn registry_keys_turn_into_executables() {
        let key =
            "C:#Program Files (x86)#Steam#steamapps#common#Counter-Strike Global Offensive#game#bin#win64#cs2.exe";
        assert_eq!(exe_of(key).as_deref(), Some("cs2.exe"));
        assert_eq!(exe_of("C:#Users#me#AppData#Local#Discord#app-1.0.9#Discord.exe").as_deref(), Some("discord.exe"));
        assert_eq!(exe_of("C:#tools#readme.txt"), None);
        assert_eq!(display_name("cs2.exe"), "Counter-Strike 2");
        assert_eq!(display_name("obs64.exe"), "OBS Studio");
        assert_eq!(display_name("audacity.exe"), "Audacity");
        assert_eq!(display_name("эфир.exe"), "эфир");
    }

    #[test]
    fn newest_entry_wins_and_active_apps_come_first() {
        let list = dedupe(vec![
            app("discord.exe", 10, false),
            app("discord.exe", 30, false),
            app("cs2.exe", 20, false),
            app("telegram.exe", 5, true),
            app("rundll32.exe", 99, false),
        ]);
        let order: Vec<_> = list.iter().map(|a| (a.exe.as_str(), a.last_used)).collect();
        assert_eq!(order, [("telegram.exe", 5), ("discord.exe", 30), ("cs2.exe", 20)]);
    }

    #[test]
    fn snapshot_lookup_matches_lower_case_names() {
        let snap = vec![("cs2.exe".to_string(), 10), ("discord.exe".to_string(), 11), ("discord.exe".to_string(), 12)];
        assert_eq!(pids_of(&snap, "discord.exe"), [11, 12]);
        assert!(pids_of(&snap, "obs64.exe").is_empty());
    }

    #[test]
    fn this_process_shows_up_in_a_snapshot() {
        let me = std::process::id();
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        let all = processes();
        assert!(all.iter().any(|(n, pid)| *pid == me && *n == name), "{name} ({me}) not in the snapshot");
        assert!(all.len() > 10 && all.iter().any(|(n, _)| n == "csrss.exe"), "only {} processes", all.len());
    }

    /// The direct query must see what Toolhelp sees: same names, same pids.
    #[test]
    fn the_snapshot_matches_toolhelp() {
        let toolhelp = || unsafe {
            let mut all = Vec::new();
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap();
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(MAX_PATH as usize);
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
                all.push((name, entry.th32ProcessID));
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
            all
        };
        let (before, ours, after) = (toolhelp(), processes(), toolhelp());
        // Processes come and go between the snapshots; anything alive through all three
        // must be in ours under the same name. Pid 0 is unnamed here, "[System Process]" there.
        let steady: Vec<_> = before.iter().filter(|p| p.1 != 0 && after.contains(p)).collect();
        assert!(steady.len() > 10);
        for p in steady {
            assert!(ours.contains(p), "{p:?} missing");
        }
    }

    /// `cargo test --release sync_cost -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark on this machine"]
    fn sync_cost() {
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let snapshot = processes();
            for a in recent().iter().take(8) {
                pids_of(&snapshot, &a.exe);
            }
        }
        println!("one sync pass: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }

    #[test]
    #[ignore = "reads this machine's registry"]
    fn lists_this_machine() {
        let snapshot = processes();
        for a in recent().iter().take(12) {
            println!("{:<24} active={} running={:?}", a.exe, a.active, pids_of(&snapshot, &a.exe));
        }
    }
}
