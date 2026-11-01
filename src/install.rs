//! Per-user install and uninstall, no administrator rights needed. The download is the
//! setup: run from anywhere else, it offers to copy itself to %LOCALAPPDATA%\Programs\Goby,
//! add a Start menu shortcut and an entry under Settings > Apps, whose Uninstall runs
//! `goby.exe --uninstall`.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteTreeW, RegSetValueExW,
};
use windows::Win32::System::Threading::{INFINITE, WaitForSingleObject};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, SW_HIDE, WM_CLOSE};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, Result, w};

use crate::lang::pick;
use crate::{apps, engine, policy, settings::Settings};

const UNINSTALL_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Goby");
const AUTHOR: &str = "Azer Sadykhzadeh";
const WEBSITE: &str = "https://azer.one";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as _));
        path
    }
}

/// `%LOCALAPPDATA%\Programs\Goby`, where per-user programs belong.
pub fn install_dir() -> Option<PathBuf> {
    known_folder(&FOLDERID_UserProgramFiles).map(|d| d.join("Goby"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches('\\'))
}

/// True when this process is the installed copy (or a development build), so it just runs.
pub fn is_installed_copy() -> bool {
    let (Ok(exe), Some(dir)) = (std::env::current_exe(), install_dir()) else { return true };
    exe.parent().is_some_and(|p| same_dir(p, &dir))
}

fn shortcut_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = known_folder(&FOLDERID_Programs) {
        v.push(p.join("Goby.lnk"));
    }
    if let Some(d) = known_folder(&FOLDERID_Desktop) {
        v.push(d.join("Goby.lnk"));
    }
    v
}

fn make_shortcut(link: &Path, target: &Path) -> Result<()> {
    unsafe {
        let shell: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell.SetPath(&HSTRING::from(target.as_os_str()))?;
        shell.SetDescription(w!("Goby voice changer"))?;
        shell.SetIconLocation(&HSTRING::from(target.as_os_str()), 0)?;
        shell.cast::<IPersistFile>()?.Save(&HSTRING::from(link.as_os_str()), true)
    }
}

fn set_string(key: HKEY, name: &str, value: &str) {
    let data: Vec<u8> = value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect();
    unsafe {
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&data));
    }
}

fn set_dword(key: HKEY, name: &str, value: u32) {
    unsafe {
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
    }
}

/// The values Settings > Apps shows for Goby; pure, so it can be tested.
pub fn uninstall_entry(exe: &Path, size_kb: u32) -> Vec<(&'static str, Option<String>, u32)> {
    let exe = exe.display().to_string();
    let dir = Path::new(&exe).parent().map(|p| p.display().to_string()).unwrap_or_default();
    vec![
        ("DisplayName", Some("Goby".into()), 0),
        ("DisplayVersion", Some(VERSION.into()), 0),
        ("Publisher", Some(AUTHOR.into()), 0),
        ("URLInfoAbout", Some(WEBSITE.into()), 0),
        ("HelpLink", Some(WEBSITE.into()), 0),
        ("DisplayIcon", Some(format!("{exe},0")), 0),
        ("InstallLocation", Some(dir), 0),
        ("UninstallString", Some(format!("\"{exe}\" --uninstall")), 0),
        ("QuietUninstallString", Some(format!("\"{exe}\" --uninstall --quiet")), 0),
        ("EstimatedSize", None, size_kb),
        ("NoModify", None, 1),
        ("NoRepair", None, 1),
    ]
}

fn write_uninstall_entry(exe: &Path) {
    let size_kb = std::fs::metadata(exe).map(|m| m.len().div_ceil(1024) as u32).unwrap_or(0);
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

/// Asks the running Goby windows to close (they save their settings) and waits a moment.
fn close_running() {
    let deadline = Instant::now() + Duration::from_secs(4);
    while let Ok(hwnd) = unsafe { FindWindowW(w!("GobyWindow"), PCWSTR::null()) } {
        if hwnd.is_invalid() || Instant::now() > deadline {
            break;
        }
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Copies this executable into the install folder and registers it. An older copy that is
/// still running is renamed out of the way: Windows allows renaming a running executable.
pub fn install(desktop_shortcut: bool) -> std::io::Result<PathBuf> {
    // Shortcuts are COM objects; this may run before the window has set COM up.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let dir = install_dir().ok_or_else(|| std::io::Error::other("no program folder"))?;
    std::fs::create_dir_all(&dir)?;
    let target = dir.join("goby.exe");
    close_running();
    if target.exists() && std::fs::remove_file(&target).is_err() {
        let _ = std::fs::remove_file(dir.join("goby.old"));
        std::fs::rename(&target, dir.join("goby.old"))?;
    }
    std::fs::copy(std::env::current_exe()?, &target)?;
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

/// The first-run question, as a native task dialog with command links.
pub fn ask() -> Choice {
    let installed = install_dir().is_some_and(|d| d.join("goby.exe").exists());
    let install_text = if installed {
        HSTRING::from(pick(
            "Обновить Goby\nЗаменить установленную копию этой версией",
            "Update Goby\nReplace the installed copy with this version",
        ))
    } else {
        HSTRING::from(pick(
            "Установить Goby\nВ меню «Пуск» и «Приложения», без прав администратора",
            "Install Goby\nTo the Start menu and Settings > Apps, no admin rights needed",
        ))
    };
    let run_text =
        HSTRING::from(pick("Просто запустить\nНичего не устанавливая", "Just run it\nWithout installing anything"));
    let buttons = [
        TASKDIALOG_BUTTON { nButtonID: INSTALL, pszButtonText: PCWSTR(install_text.as_ptr()) },
        TASKDIALOG_BUTTON { nButtonID: RUN_ONCE, pszButtonText: PCWSTR(run_text.as_ptr()) },
    ];
    let title = HSTRING::from(format!("Goby {VERSION}"));
    let heading = HSTRING::from(pick("Изменитель голоса для игр и чатов", "A voice changer for games and chats"));
    let desktop = HSTRING::from(pick("Ярлык на рабочем столе", "Desktop shortcut"));
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
    let heading = pick("Не получилось установить Goby", "Goby could not be installed");
    let text = format!("{error}\n\n{}", pick("Goby запустится без установки.", "Goby will run without installing."));
    message(heading, &text, false);
}

/// Runs this executable elevated with `args` and waits for it (one UAC prompt).
fn run_elevated(args: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    let (exe, args) = (HSTRING::from(exe.as_os_str()), HSTRING::from(args));
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
    if !quiet
        && !message(
            pick("Удалить Goby?", "Uninstall Goby?"),
            pick(
                "Программы, которые слышали Goby, получат обратно обычный микрофон, а кабелю вернётся прежнее имя.",
                "Programs that heard Goby get your own microphone back, and the cable gets its old name back.",
            ),
            true,
        )
    {
        return;
    }
    close_running();

    // Hand every chosen program back to the default mic while it is running; Windows keeps
    // the choice per program, so a program that is closed now keeps hearing the cable.
    let settings = Settings::load();
    let snapshot = apps::processes();
    let mut left = Vec::new();
    for exe in &settings.routed {
        let pids = apps::pids_of(&snapshot, exe);
        if !pids.iter().any(|pid| policy::route_app(*pid, None).is_ok()) {
            left.push(apps::display_name(exe));
        }
    }
    // Give the cable its VB-Audio names back.
    if let Some((play, record)) = engine::cable()
        && play.name.starts_with("Goby")
    {
        run_elevated(&format!("--name-cable \"{}\" \"{}\" \"CABLE Input\" \"CABLE Output\"", play.id, record.id));
    }

    for link in shortcut_paths() {
        let _ = std::fs::remove_file(link);
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
    if let Some(dir) = Settings::dir() {
        let _ = std::fs::remove_dir_all(dir);
    }
    // The running executable cannot delete itself; a hidden shell removes the folder once
    // this process has exited.
    if let Some(dir) = install_dir() {
        let script = format!("ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
        let _ = std::process::Command::new("cmd.exe")
            // Passed verbatim: escaped quotes would reach cmd.exe as \" and break the path.
            .raw_arg(format!("/d /c {script}"))
            .current_dir(std::env::temp_dir())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    if !quiet {
        let note = if left.is_empty() {
            pick("Goby удалён.", "Goby is uninstalled.").to_string()
        } else {
            format!(
                "{} {}",
                pick(
                    "Goby удалён. Эти программы были закрыты и пока слушают кабель; выберите им микрофон в «Микшере громкости»:",
                    "Goby is uninstalled. These programs were closed and still listen to the cable; pick their mic in the Volume mixer:"
                ),
                left.join(", ")
            )
        };
        message("Goby", &note, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_entry_names_the_author_and_uninstalls_quietly_too() {
        let entry = uninstall_entry(Path::new(r"C:\Users\me\AppData\Local\Programs\Goby\goby.exe"), 460);
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
        assert!(same_dir(Path::new(r"C:\Users\Me\Programs\Goby\"), Path::new(r"c:\users\me\programs\goby")));
        assert!(!same_dir(Path::new(r"C:\Goby"), Path::new(r"C:\Goby2")));
    }
}
