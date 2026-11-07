//! What the window asks of Windows: WASAPI devices and the engine, the programs' microphone
//! routing, the shell, the registry and updates.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
use windows::core::{HSTRING, PCWSTR, w};

use super::updates::{self, Inbox};
use crate::engine::{self, Engine, Shared};
use crate::gfx::Gfx;
use crate::host::{AppState, Endpoint, Host, Link, MicApp, Pinned, SCOPE_LEN};
use crate::settings::Settings;
use crate::{apps, hotkeys, install, policy};

pub struct Desktop {
    pub hwnd: HWND,
    pub gfx: Option<Gfx>,
    pub mica: bool,
    /// Milliseconds between timer ticks: a frame while shown, much slower otherwise.
    pub pace: u32,
    started: Instant,
    shared: Arc<Shared>,
    engine: Option<Engine>,
    /// Processes already pinned to Goby, so each is asked only once, and the cable path
    /// they were pinned to: a different cable starts the cache over.
    pinned: Vec<u32>,
    pinned_to: String,
    /// The notification-area icon is up, and the tip it shows.
    pub tray_icon: bool,
    pub tray_tip: String,
    /// A newer verified release, and the mailbox of the threads that check and download.
    pub manifest: Option<crate::update::Manifest>,
    pub inbox: Arc<Mutex<Inbox>>,
}

impl Desktop {
    pub fn new(hwnd: HWND) -> Desktop {
        Desktop {
            hwnd,
            gfx: None,
            mica: false,
            pace: 0,
            started: Instant::now(),
            shared: Arc::new(Shared::default()),
            engine: None,
            pinned: Vec::new(),
            pinned_to: String::new(),
            tray_icon: false,
            tray_tip: String::new(),
            manifest: None,
            inbox: updates::inbox(),
        }
    }

    fn shell_open(&self, target: PCWSTR) {
        unsafe {
            ShellExecuteW(Some(self.hwnd), w!("open"), target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }
}

impl Host for Desktop {
    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    fn animations(&self) -> bool {
        super::animations_on()
    }

    fn language(&self) -> String {
        install::display_language()
    }

    fn save(&self, settings: &Settings) {
        settings.save();
    }

    fn devices(&mut self, capture: bool) -> Vec<Endpoint> {
        engine::list(capture)
    }

    fn default_device(&mut self, capture: bool) -> Option<Endpoint> {
        engine::default_endpoint(capture)
    }

    fn cable(&mut self) -> Option<(Endpoint, Endpoint)> {
        engine::cable()
    }

    fn name_cable(&mut self, play: &Endpoint, record: &Endpoint) {
        let exe = crate::files::current_exe().unwrap_or_default();
        let args = format!("--name-cable \"{}\" \"{}\"", play.id, record.id);
        let (exe, args) = (HSTRING::from(exe), HSTRING::from(args));
        unsafe {
            ShellExecuteW(
                Some(self.hwnd),
                w!("runas"),
                PCWSTR(exe.as_ptr()),
                PCWSTR(args.as_ptr()),
                PCWSTR::null(),
                SW_HIDE,
            );
        }
    }

    fn start(&mut self, mic: &str, exclusive: bool, outputs: &[String]) -> Result<(), String> {
        self.engine = Some(Engine::start(mic, exclusive, outputs, self.shared.clone())?);
        Ok(())
    }

    fn stop(&mut self) {
        self.engine = None;
        self.shared.failed.store(false, Relaxed);
    }

    fn running(&self) -> bool {
        self.engine.is_some()
    }

    fn failed(&self) -> bool {
        self.shared.failed.load(Relaxed)
    }

    fn exclusive(&self) -> bool {
        self.shared.mic_exclusive.load(Relaxed)
    }

    fn voice(&self, voice: usize, tone: f32, volume: f32) {
        self.shared.set(voice, tone, volume);
    }

    fn mic(&self, gain_db: f32, gate_db: Option<f32>, dry_mix: f32, enabled: bool) {
        self.shared.set_mic(gain_db, gate_db, dry_mix, enabled);
    }

    fn latency_ms(&self) -> f32 {
        self.shared.latency_ms()
    }

    fn scope(&mut self, raw: &mut [f32; SCOPE_LEN], voiced: &mut [f32; SCOPE_LEN]) -> bool {
        self.shared.scope(raw, voiced);
        true
    }

    fn recent_apps(&mut self) -> Vec<MicApp> {
        apps::recent()
    }

    fn pin(
        &mut self,
        record: &str,
        routed: &[String],
        unroute: &mut Vec<String>,
        paths: &mut Vec<(String, String)>,
        watching: bool,
    ) -> Pinned {
        let ours = policy::capture_interface_path(record);
        if ours != self.pinned_to {
            self.pinned.clear();
            self.pinned_to = ours.clone();
        }
        // Listing every process takes 4 to 6 ms of kernel time; with no program to pin or
        // hand back, nothing below looks at it.
        let idle = routed.is_empty() && unroute.is_empty();
        let snapshot = if idle { Vec::new() } else { apps::processes() };
        self.pinned.retain(|pid| snapshot.iter().any(|(_, p)| p == pid));

        // A chosen program Windows has not listed yet still has an icon on disk.
        for exe in routed {
            if !paths.iter().any(|(e, _)| e == exe)
                && let Some(path) = apps::pids_of(&snapshot, exe).into_iter().find_map(apps::exe_path)
            {
                paths.push((exe.clone(), path));
            }
        }
        // Which microphone each chosen program really records from at this moment: the
        // per-app default only reaches programs that use the default, and one with its own
        // microphone setting is the case worth saying out loud. Asking every microphone for
        // its sessions takes 3 to 5 ms, and only the Programs page shows the answer.
        let live = watching.then(|| {
            let sessions = if routed.is_empty() { Vec::new() } else { engine::recording() };
            let (mut live, mut elsewhere) = (Vec::new(), Vec::new());
            for exe in routed {
                let pids = apps::pids_of(&snapshot, exe);
                let mine = sessions.iter().filter(|(pid, _)| pids.contains(pid));
                live.push(mine.clone().any(|(_, ep)| ep.id == record));
                elsewhere.push(
                    mine.into_iter()
                        .find(|(_, ep)| ep.id != record)
                        .map(|(_, ep)| super::super::short(&ep.name).to_string()),
                );
            }
            (live, elsewhere)
        });

        let mut states = Vec::with_capacity(routed.len());
        for exe in routed {
            let pids = apps::pids_of(&snapshot, exe);
            let mut pinned = pids.iter().any(|p| self.pinned.contains(p));
            // One success covers the whole program; Windows stores the choice per program.
            for pid in pids.iter().filter(|_| !pinned) {
                let already = policy::app_route(*pid).is_ok_and(|now| now.eq_ignore_ascii_case(&ours));
                if already || policy::route_app(*pid, Some(record)).is_ok() {
                    pinned = true;
                    self.pinned.push(*pid);
                    break;
                }
            }
            states.push(match (pids.is_empty(), pinned) {
                (true, _) => AppState::Waiting,
                (false, true) => AppState::OnGoby,
                (false, false) => AppState::Pending,
            });
        }

        // Hand removed programs back to the default mic once they run; a program that is
        // no longer on Goby has nothing to undo.
        unroute.retain(|exe| {
            for pid in apps::pids_of(&snapshot, exe) {
                match policy::app_route(pid) {
                    Ok(now) if !now.eq_ignore_ascii_case(&ours) => return false,
                    Ok(_) if policy::route_app(pid, None).is_ok() => return false,
                    _ => {}
                }
            }
            true
        });
        Pinned { states, live }
    }

    fn repin(&mut self) {
        self.pinned.clear();
    }

    fn icon(&self, path: &str, px: u32) -> Option<Vec<u8>> {
        apps::icon_pixels(path, px as i32)
    }

    fn open(&mut self, link: Link) {
        match link {
            Link::Author => self.shell_open(w!("https://azer.one")),
            Link::Cable => self.shell_open(w!("https://vb-audio.com/Cable/")),
            Link::Downloads => install::launch(w!("explorer.exe"), "shell:Downloads", None, SW_SHOWNORMAL),
        }
    }

    fn capture(&mut self, on: bool) {
        unsafe {
            if on {
                SetCapture(self.hwnd);
            } else {
                let _ = ReleaseCapture();
            }
        }
    }

    fn autostart(&self) -> bool {
        install::starts_with_windows()
    }

    fn set_autostart(&mut self, on: bool) {
        install::set_start_with_windows(on);
    }

    fn hotkeys(&mut self, on: bool) -> bool {
        hotkeys::unregister(self.hwnd);
        !on || hotkeys::register(self.hwnd)
    }

    fn check_for_update(&mut self, settings: &Settings, delay: Duration) {
        updates::check(self, settings, delay);
    }

    fn update(&mut self) {
        updates::install(self);
    }

    fn installed(&self) -> bool {
        install::is_installed_copy()
    }
}
