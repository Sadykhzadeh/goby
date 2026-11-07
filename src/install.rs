//! Per-user install and uninstall, no administrator rights needed. The download is the
//! setup: run from anywhere else, it offers to copy itself to %LOCALAPPDATA%\Programs\Goby,
//! add a Start menu shortcut and an entry under Settings > Apps, whose Uninstall runs
//! `goby.exe --uninstall`.

use std::borrow::Cow;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_ANY, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, SHOW_WINDOW_CMD, SW_HIDE, WM_CLOSE};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, Result, w};

use crate::files::{self, parent};
use crate::lang::{self, tr, trf};
use crate::{apps, engine, policy, settings::Settings};

const UNINSTALL_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Goby");
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
/// Where Task Manager's Startup apps page keeps the entries it has switched off.
const APPROVED_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
const AUTHOR: &str = "Azer Sadykhzadeh";
const WEBSITE: &str = "https://azer.one";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn known_folder(id: &GUID) -> Option<String> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        path
    }
}

/// `%LOCALAPPDATA%\Programs\Goby`, where per-user programs belong.
pub fn install_dir() -> Option<String> {
    known_folder(&FOLDERID_UserProgramFiles).map(|d| format!("{d}\\Goby"))
}

fn same_dir(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// True when this process is the installed copy (or a development build), so it just runs.
pub fn is_installed_copy() -> bool {
    let (Some(exe), Some(dir)) = (files::current_exe(), install_dir()) else { return true };
    parent(&exe).is_some_and(|p| same_dir(p, &dir))
}

fn shortcut_paths() -> Vec<String> {
    let mut v = Vec::new();
    if let Some(p) = known_folder(&FOLDERID_Programs) {
        v.push(format!("{p}\\Goby.lnk"));
    }
    if let Some(d) = known_folder(&FOLDERID_Desktop) {
        v.push(format!("{d}\\Goby.lnk"));
    }
    v
}

fn make_shortcut(link: &str, target: &str) -> Result<()> {
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let target = HSTRING::from(target);
        shell.SetPath(&target)?;
        shell.SetDescription(w!("Goby voice changer"))?;
        shell.SetIconLocation(&target, 0)?;
        shell.cast::<IPersistFile>()?.Save(&HSTRING::from(link), true)
    }
}

fn set_string(key: HKEY, name: &str, value: &str) {
    // An HSTRING is already UTF-16 with its terminating NUL, which is what REG_SZ wants.
    let value = HSTRING::from(value);
    unsafe {
        let data = std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), (value.len() + 1) * 2);
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(data));
    }
}

fn set_dword(key: HKEY, name: &str, value: u32) {
    unsafe {
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
    }
}

/// The values Settings > Apps shows for Goby; pure, so it can be tested.
pub fn uninstall_entry(exe: &str, size_kb: u32) -> Vec<(&'static str, Option<Cow<'static, str>>, u32)> {
    let dir = parent(exe).unwrap_or_default().to_string();
    // The fixed values are borrowed rather than copied into a String each.
    vec![
        ("DisplayName", Some("Goby".into()), 0),
        ("DisplayVersion", Some(VERSION.into()), 0),
        ("Publisher", Some(AUTHOR.into()), 0),
        ("URLInfoAbout", Some(WEBSITE.into()), 0),
        ("HelpLink", Some(WEBSITE.into()), 0),
        ("DisplayIcon", Some(format!("{exe},0").into()), 0),
        ("InstallLocation", Some(dir.into()), 0),
        ("UninstallString", Some(format!("\"{exe}\" --uninstall").into()), 0),
        ("QuietUninstallString", Some(format!("\"{exe}\" --uninstall --quiet").into()), 0),
        ("EstimatedSize", None, size_kb),
        ("NoModify", None, 1),
        ("NoRepair", None, 1),
    ]
}

fn write_uninstall_entry(exe: &str) {
    let size_kb = files::size(exe).map_or(0, |len| len.div_ceil(1024) as u32);
    unsafe {
        let mut key = HKEY::default();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if created != ERROR_SUCCESS {
            return;
        }
        for (name, text, number) in uninstall_entry(exe, size_kb) {
            match text {
                Some(t) => set_string(key, name, &t),
                None => set_dword(key, name, number),
            }
        }
        let _ = RegCloseKey(key);
    }
}

/// What Windows runs at sign-in: Goby in the tray, without the setup question a copy
/// outside the install folder would ask.
pub fn autostart_command(exe: &str, installed: bool) -> String {
    let portable = if installed { "" } else { " --portable" };
    format!("\"{exe}\"{portable} --tray")
}

/// Task Manager marks an entry it switched off with an odd first byte.
pub fn approved(state: &[u8]) -> bool {
    state.first().is_none_or(|b| b % 2 == 0)
}

fn read_value(key: PCWSTR, name: PCWSTR) -> Option<Vec<u8>> {
    let mut size = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_ANY, None, None, Some(&mut size)) != ERROR_SUCCESS {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let read = RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            name,
            RRF_RT_ANY,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        );
        (read == ERROR_SUCCESS).then(|| {
            data.truncate(size as usize);
            data
        })
    }
}

/// Whether Windows starts Goby at sign-in.
pub fn starts_with_windows() -> bool {
    read_value(RUN_KEY, w!("Goby")).is_some() && read_value(APPROVED_KEY, w!("Goby")).is_none_or(|s| approved(&s))
}

pub fn set_start_with_windows(on: bool) {
    unsafe {
        // Switching it on here also clears an earlier "off" from Task Manager.
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED_KEY, w!("Goby"));
        if !on {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Goby"));
            return;
        }
        let Some(exe) = files::current_exe() else { return };
        let mut key = HKEY::default();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if created == ERROR_SUCCESS {
            set_string(key, "Goby", &autostart_command(&exe, is_installed_copy()));
            let _ = RegCloseKey(key);
        }
    }
}

/// Asks the running Goby windows to close (they save their settings) and waits a moment.
fn close_running() {
    let deadline = Instant::now() + Duration::from_secs(4);
    while let Ok(hwnd) = unsafe { FindWindowW(w!("GobyWindow"), PCWSTR::null()) } {
        if hwnd.is_invalid() || Instant::now() > deadline {
            break;
        }
        // Closing only hides a Goby that keeps running in the tray; versions from before
        // the tray ignore the quit message and close.
        unsafe {
            let _ = PostMessageW(Some(hwnd), crate::tray::WM_QUIT_GOBY, WPARAM(0), LPARAM(0));
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Copies this executable into the install folder and registers it. An older copy that is
/// still running is renamed out of the way: Windows allows renaming a running executable.
pub fn install(desktop_shortcut: bool) -> std::io::Result<String> {
    // Shortcuts are COM objects; this may run before the window has set COM up.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let dir = install_dir().ok_or_else(|| std::io::Error::other("no program folder"))?;
    files::create_dirs(&dir)?;
    let target = format!("{dir}\\goby.exe");
    let old = format!("{dir}\\goby.old");
    let source = files::current_exe().ok_or_else(std::io::Error::last_os_error)?;
    // Run as the installed copy itself (`--install` from the Start menu copy): nothing to
    // copy, just refresh the shortcuts and the entry.
    if !parent(&source).is_some_and(|p| same_dir(p, &dir)) {
        close_running();
        files::remove_file(&old);
        let moved = files::exists(&target) && !files::remove_file(&target);
        if moved {
            files::rename(&target, &old)?;
        }
        if let Err(e) = files::copy(&source, &target) {
            // Put the previous version back rather than leave a broken install.
            if moved {
                let _ = files::rename(&old, &target);
            }
            return Err(e);
        }
    }
    let links = shortcut_paths();
    for (i, link) in links.iter().enumerate() {
        // The first is the Start menu; the second, the desktop, only when asked for.
        if i == 0 || desktop_shortcut {
            let _ = make_shortcut(link, &target);
        }
    }
    write_uninstall_entry(&target);
    Ok(target)
}

pub enum Choice {
    Install { desktop: bool },
    RunOnce,
    Quit,
}

const INSTALL: i32 = 100;
const RUN_ONCE: i32 = 101;

/// Windows' display language, as a tag such as "ru-RU".
pub fn display_language() -> String {
    use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};
    let mut name = [0u16; 85];
    let n = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage() as u32, Some(&mut name), 0) };
    String::from_utf16_lossy(&name[..(n.max(1) - 1) as usize])
}

/// The dialogs here come before the window, which picks the language itself.
fn use_chosen_language() {
    lang::choose(&Settings::load().lang, &display_language());
}

/// The first-run question, as a native task dialog with command links.
pub fn ask() -> Choice {
    use_chosen_language();
    let installed = install_dir().is_some_and(|d| files::exists(&format!("{d}\\goby.exe")));
    let install_text = if installed { HSTRING::from(tr!("setup.update")) } else { HSTRING::from(tr!("setup.install")) };
    let run_text = HSTRING::from(tr!("setup.run"));
    let buttons = [
        TASKDIALOG_BUTTON { nButtonID: INSTALL, pszButtonText: PCWSTR(install_text.as_ptr()) },
        TASKDIALOG_BUTTON { nButtonID: RUN_ONCE, pszButtonText: PCWSTR(run_text.as_ptr()) },
    ];
    let title = HSTRING::from(format!("Goby {VERSION}"));
    let heading = HSTRING::from(tr!("setup.heading"));
    let desktop = HSTRING::from(tr!("setup.desktop"));
    let footer = HSTRING::from(format!("{AUTHOR} · <a href=\"{WEBSITE}\">azer.one</a>"));
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_USE_COMMAND_LINKS
            | TDF_ALLOW_DIALOG_CANCELLATION
            | TDF_ENABLE_HYPERLINKS
            | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CLOSE_BUTTON,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(heading.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: INSTALL,
        pszVerificationText: PCWSTR(desktop.as_ptr()),
        pszFooter: PCWSTR(footer.as_ptr()),
        pfCallback: Some(open_links),
        ..Default::default()
    };
    let (mut button, mut checked) = (0i32, BOOL(0));
    if unsafe { TaskDialogIndirect(&config, Some(&mut button), None, Some(&mut checked)) }.is_err() {
        return Choice::RunOnce;
    }
    match button {
        INSTALL => Choice::Install { desktop: checked.as_bool() },
        RUN_ONCE => Choice::RunOnce,
        _ => Choice::Quit,
    }
}

/// Opens the footer link in the browser.
unsafe extern "system" fn open_links(
    _: HWND,
    msg: TASKDIALOG_NOTIFICATIONS,
    _: WPARAM,
    lp: LPARAM,
    _: isize,
) -> windows::core::HRESULT {
    if msg == TDN_HYPERLINK_CLICKED {
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(lp.0 as *const u16),
                PCWSTR::null(),
                PCWSTR::null(),
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
        }
    }
    windows::core::HRESULT(0)
}

fn message(heading: &str, text: &str, question: bool) -> bool {
    let (title, heading, text) = (HSTRING::from("Goby"), HSTRING::from(heading), HSTRING::from(text));
    let config = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        dwCommonButtons: if question { TDCBF_YES_BUTTON | TDCBF_NO_BUTTON } else { TDCBF_OK_BUTTON },
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(heading.as_ptr()),
        pszContent: PCWSTR(text.as_ptr()),
        ..Default::default()
    };
    let mut button = 0i32;
    unsafe { TaskDialogIndirect(&config, Some(&mut button), None, None) }.is_ok() && button == 6 // IDYES
}

/// Tells the user why installing did not work; Goby then runs without installing.
pub fn failed(error: &std::io::Error) {
    let heading = tr!("setup.failed");
    let text = trf!("setup.failed.desc", &[("error", &error.to_string())]);
    message(heading, &text, false);
}

/// Runs this executable elevated with `args` and waits for it (one UAC prompt).
fn run_elevated(args: &str) {
    let Some(exe) = files::current_exe() else { return };
    let (exe, args) = (HSTRING::from(exe), HSTRING::from(args));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if ShellExecuteExW(&mut info).is_ok() && !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = CloseHandle(info.hProcess);
        }
    }
}

/// `goby.exe --uninstall [--quiet]`, as Settings > Apps runs it.
pub fn uninstall(quiet: bool) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if !quiet {
        use_chosen_language();
    }
    if !quiet && !message(tr!("uninstall.ask"), tr!("uninstall.ask.desc"), true) {
        return;
    }
    close_running();

    // Hand every chosen program back to the default mic while it is running; Windows keeps
    // the choice per program, so a program that is closed now keeps hearing the cable.
    let settings = Settings::load();
    let snapshot = apps::processes();
    let mut left = Vec::new();
    for exe in settings.routed.iter().chain(&settings.unroute) {
        let pids = apps::pids_of(&snapshot, exe);
        if !pids.iter().any(|pid| policy::route_app(*pid, None).is_ok()) {
            left.push(crate::host::display_name(exe));
        }
    }
    // Give the cable its VB-Audio names back.
    if let Some((play, record)) = engine::cable()
        && play.name.starts_with("Goby")
    {
        run_elevated(&format!("--name-cable \"{}\" \"{}\" \"CABLE Input\" \"CABLE Output\"", play.id, record.id));
    }

    for link in shortcut_paths() {
        files::remove_file(&link);
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
    set_start_with_windows(false);
    if let Some(dir) = Settings::dir() {
        files::remove_dir_all(&dir);
    }
    if !quiet {
        let note = if left.is_empty() {
            tr!("uninstall.done").to_string()
        } else {
            trf!("uninstall.done.left", &[("programs", &left.join(", "))])
        };
        message("Goby", &note, false);
    }
    remove_folder_after_exit();
}

/// The running executable cannot delete itself: a hidden shell retries for up to half a
/// minute until this process has exited and the folder can go.
fn remove_folder_after_exit() {
    let Some(dir) = install_dir() else { return };
    let script = format!(
        "for /l %i in (1,1,30) do (ping -n 2 127.0.0.1 >nul & rmdir /s /q \"{dir}\" 2>nul & if not exist \"{dir}\" exit /b)"
    );
    // Started outside the folder it deletes, and hidden.
    launch(w!("cmd.exe"), &format!("/d /c {script}"), files::temp_dir().as_deref(), SW_HIDE);
}

/// Starts a program and leaves it running. Through the shell rather than std::process,
/// whose process plumbing would add about 30 KB to the executable for two launches.
/// The arguments reach the program verbatim.
pub fn launch(file: impl windows::core::Param<PCWSTR>, args: &str, dir: Option<&str>, show: SHOW_WINDOW_CMD) {
    let args = HSTRING::from(args);
    let dir = dir.map(HSTRING::from);
    let dir = dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr()));
    unsafe {
        ShellExecuteW(None, w!("open"), file, &args, dir, show);
    }
}

/// Removes the previous version an update moved aside, once it is no longer running.
pub fn tidy() {
    if let Some(dir) = install_dir() {
        files::remove_file(&format!("{dir}\\goby.old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_starts_goby_in_the_tray_without_the_setup_question() {
        let exe = r"C:\Users\a\AppData\Local\Programs\Goby\goby.exe";
        assert_eq!(autostart_command(exe, true), r#""C:\Users\a\AppData\Local\Programs\Goby\goby.exe" --tray"#);
        assert_eq!(autostart_command(r"D:\tools\goby.exe", false), r#""D:\tools\goby.exe" --portable --tray"#);
    }

    #[test]
    fn task_manager_switches_an_entry_off_with_an_odd_first_byte() {
        assert!(approved(&[2, 0, 0, 0]));
        assert!(approved(&[6, 0, 0, 0]));
        assert!(!approved(&[3, 0, 0, 0, 1, 2, 3, 4]));
        assert!(approved(&[]));
    }

    #[test]
    fn the_apps_entry_names_the_author_and_uninstalls_quietly_too() {
        let entry = uninstall_entry(r"C:\Users\me\AppData\Local\Programs\Goby\goby.exe", 460);
        let get = |k: &str| entry.iter().find(|(n, _, _)| *n == k).and_then(|(_, v, _)| v.clone());
        assert_eq!(get("Publisher").as_deref(), Some("Azer Sadykhzadeh"));
        assert_eq!(get("URLInfoAbout").as_deref(), Some("https://azer.one"));
        assert_eq!(get("InstallLocation").as_deref(), Some(r"C:\Users\me\AppData\Local\Programs\Goby"));
        assert_eq!(
            get("UninstallString").as_deref(),
            Some(r#""C:\Users\me\AppData\Local\Programs\Goby\goby.exe" --uninstall"#)
        );
        assert!(get("QuietUninstallString").unwrap().ends_with("--uninstall --quiet"));
        assert_eq!(entry.iter().find(|(n, _, _)| *n == "EstimatedSize").map(|e| e.2), Some(460));
    }

    #[test]
    fn install_folder_is_per_user() {
        let dir = install_dir().expect("known folder");
        let local = std::env::var("LOCALAPPDATA").unwrap();
        assert!(dir.starts_with(&local), "{dir:?}");
        assert!(dir.ends_with(r"Programs\Goby"));
    }

    #[test]
    fn folders_compare_without_case_or_trailing_slash() {
        assert!(same_dir(r"C:\Users\Me\Programs\Goby\", r"c:\users\me\programs\goby"));
        assert!(!same_dir(r"C:\Goby", r"C:\Goby2"));
    }
}
