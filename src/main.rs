#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod dsp;
mod engine;
mod fx;
mod gfx;
mod hotkeys;
mod icon;
mod install;
mod lang;
mod mic;
mod policy;
mod ring;
mod settings;
mod theme;
mod tray;
mod ui;
mod voices;

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        // Relaunched elevated to (re)name the virtual cable; does only that.
        ["--name-cable", render, capture] => policy::name_cable(render, capture, ("Goby", "Goby Mic")),
        ["--name-cable", render, capture, play_name, record_name] => {
            policy::name_cable(render, capture, (play_name, record_name))
        }
        // Silent install for scripts and package managers.
        ["--install", rest @ ..] => {
            if install::install(rest.contains(&"--desktop")).is_err() {
                std::process::exit(1);
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
