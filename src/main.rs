#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
#[cfg(test)]
mod demo;
mod draw;
mod engine;
mod files;
mod gfx;
mod host;
mod hotkeys;
mod icon;
mod install;
mod lang;
mod policy;
mod settings;
mod theme;
mod tray;
mod ui;
mod update;
mod voices;

// The voices themselves live in crates/dsp, built at full speed whatever this crate is built for.
use goby_dsp::{dsp, mic, ring, stream};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{
    CreateThread, INFINITE, STACK_SIZE_PARAM_IS_A_RESERVATION, WaitForSingleObject,
};

type Work = Box<dyn FnOnce() + Send>;

/// A thread started by `spawn`; dropping it lets the thread run on unwatched.
struct Thread(HANDLE);

impl Thread {
    fn join(self) {
        unsafe { WaitForSingleObject(self.0, INFINITE) };
    }
}

impl Drop for Thread {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Starts a thread straight through Win32: std's thread::spawn brings start-up, naming
/// and result plumbing that nothing here uses. The stack is 2 MiB, as std's would be.
fn spawn(work: Work) -> Thread {
    unsafe extern "system" fn start(work: *mut std::ffi::c_void) -> u32 {
        unsafe { Box::from_raw(work as *mut Work)() };
        0
    }
    let work = Box::into_raw(Box::new(work));
    let flags = STACK_SIZE_PARAM_IS_A_RESERVATION;
    match unsafe { CreateThread(None, 2 << 20, Some(start), Some(work as _), flags, None) } {
        Ok(handle) => Thread(handle),
        Err(e) => {
            drop(unsafe { Box::from_raw(work) });
            panic!("failed to spawn thread: {e}")
        }
    }
}

/// The command-line arguments after the program's own name. Windows splits them itself;
/// std::env::args would bring its own parser along.
fn args() -> Vec<String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::System::Environment::GetCommandLineW;
    use windows::Win32::UI::Shell::CommandLineToArgvW;
    let mut n = 0;
    unsafe {
        let argv = CommandLineToArgvW(GetCommandLineW(), &mut n);
        if argv.is_null() {
            return Vec::new();
        }
        let args = std::slice::from_raw_parts(argv, n as usize);
        let args = args.iter().skip(1).map(|a| String::from_utf16_lossy(a.as_wide())).collect();
        LocalFree(Some(HLOCAL(argv as _)));
        args
    }
}

fn main() -> windows::core::Result<()> {
    let args = args();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        // Relaunched elevated to (re)name the virtual cable; does only that.
        ["--name-cable", render, capture] => policy::name_cable(render, capture, ("Goby", "Goby Mic")),
        ["--name-cable", render, capture, play_name, record_name] => {
            policy::name_cable(render, capture, (play_name, record_name))
        }
        // Silent install for scripts and package managers; with `--run`, as an update does
        // it, the installed copy starts afterwards.
        ["--install", rest @ ..] => {
            let Ok(exe) = install::install(rest.contains(&"--desktop")) else {
                std::process::exit(1);
            };
            if rest.contains(&"--run") {
                let exe = windows::core::HSTRING::from(exe.as_os_str());
                install::launch(&exe, "", None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
            }
            Ok(())
        }
        ["--uninstall", rest @ ..] => {
            install::uninstall(rest.contains(&"--quiet"));
            Ok(())
        }
        // Debug builds and `--portable` skip the setup question.
        rest if cfg!(debug_assertions) || rest.contains(&"--portable") || install::is_installed_copy() => {
            install::tidy();
            update::tidy();
            ui::run(rest.contains(&"--tray"))
        }
        _ => match install::ask() {
            install::Choice::Install { desktop } => {
                match install::install(desktop) {
                    Ok(exe) => {
                        let exe = windows::core::HSTRING::from(exe.as_os_str());
                        install::launch(&exe, "", None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
                    }
                    Err(e) => {
                        install::failed(&e);
                        ui::run(false)?;
                    }
                }
                Ok(())
            }
            install::Choice::RunOnce => ui::run(false),
            install::Choice::Quit => Ok(()),
        },
    }
}
