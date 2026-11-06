//! Which programs use the microphone. Windows keeps this list itself for the privacy page
//! (Settings > Privacy > Microphone > recent activity); we read the same registry keys.

use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, MAX_PATH};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD, RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::core::{PWSTR, w};

#[derive(Clone, Debug, PartialEq)]
pub struct MicApp {
    /// Lower-case executable name, e.g. "cs2.exe": stable across updates that move the folder.
    pub exe: String,
    /// What to show: the executable name without ".exe".
    pub name: String,
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
    let stem = exe.strip_suffix(".exe").unwrap_or(exe);
    let mut chars = stem.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars).collect())
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
            found.push(MicApp { name: display_name(&exe), exe, last_used: start, active: start > 0 && stop == 0 });
        }
        let _ = RegCloseKey(root);
    }
    dedupe(found)
}

/// Every running process as (lower-case exe name, pid), from one snapshot.
pub fn processes() -> Vec<(String, u32)> {
    let mut all = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return all };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut entry).is_ok();
        while ok {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(MAX_PATH as usize);
            all.push((String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase(), entry.th32ProcessID));
            ok = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    all
}

/// Process ids of `exe` (lower-case file name) in a `processes()` snapshot.
pub fn pids_of(snapshot: &[(String, u32)], exe: &str) -> Vec<u32> {
    snapshot.iter().filter(|(name, _)| name == exe).map(|(_, pid)| *pid).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, last_used: u64, active: bool) -> MicApp {
        MicApp { exe: exe.into(), name: display_name(exe), last_used, active }
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
        assert_eq!(display_name("эфир.exe"), "Эфир");
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
        assert!(processes().iter().any(|(_, pid)| *pid == me));
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
