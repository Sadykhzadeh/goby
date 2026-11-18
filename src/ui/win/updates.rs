//! Updates in the window: a daily check in the background, and handing a verified download
//! to the installer. The checking itself, and everything that makes a download
//! trustworthy, is in `crate::update`; the card that offers it is shared with the website.

use crate::files::File;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, PostMessageW, SW_SHOWNORMAL, WM_APP};
use windows::core::{HSTRING, w};

use super::desktop::Desktop;
use super::{Win, tray};
use crate::install;
use crate::lang::tr;
use crate::settings::Settings;
use crate::ui::Updating;
use crate::update::{self, Manifest};

/// Sent by the background threads when a check or a download has finished.
pub(super) const WM_UPDATE_CHECKED: u32 = WM_APP + 3;
pub(super) const WM_UPDATE_READY: u32 = WM_APP + 4;

/// A day, give or take the hour a computer happens to start at.
const EVERY: u64 = 20 * 60 * 60;

#[derive(Default)]
pub struct Inbox {
    checked: Option<Option<Manifest>>,
    downloaded: Option<Option<(String, File)>>,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether a check is due. A last check that lies in the future, as after the clock was set
/// back, is due at once: a check then records the real time, so the wait starts over.
fn due(now: u64, last: u64) -> bool {
    now < last || now - last >= EVERY
}

/// Runs `work` on a thread of its own and wakes the window with `msg` when it is done.
fn in_background(hwnd: HWND, msg: u32, work: Box<dyn FnOnce() + Send>) {
    let raw = hwnd.0 as isize;
    crate::spawn(Box::new(move || {
        work();
        unsafe {
            let _ = PostMessageW(Some(HWND(raw as _)), msg, WPARAM(0), LPARAM(0));
        }
    }));
}

/// Starts the daily check, a little after start-up so it never competes with the audio.
pub(super) fn check(host: &mut Desktop, settings: &Settings, delay: Duration) {
    if !settings.updates || !due(now(), settings.update_checked) {
        return;
    }
    let inbox = host.inbox.clone();
    in_background(
        host.hwnd,
        WM_UPDATE_CHECKED,
        Box::new(move || {
            std::thread::sleep(delay);
            let found = update::check();
            if let Ok(mut inbox) = inbox.lock() {
                inbox.checked = Some(found);
            }
        }),
    );
}

/// The installed copy downloads and installs the update; a portable one, which Goby does
/// not manage, is sent to the release page instead.
pub(super) fn install(host: &mut Desktop) {
    let Some(m) = host.manifest.clone() else { return };
    if !install::is_installed_copy() {
        install::launch(w!("https://github.com/Sadykhzadeh/goby/releases/latest"), "", None, SW_SHOWNORMAL);
        return;
    }
    let inbox = host.inbox.clone();
    in_background(
        host.hwnd,
        WM_UPDATE_READY,
        Box::new(move || {
            let file = update::download(&m);
            if let Ok(mut inbox) = inbox.lock() {
                inbox.downloaded = Some(file);
            }
        }),
    );
}

impl Win {
    pub(super) fn update_checked(&mut self) {
        let Some(found) = self.host.inbox.lock().ok().and_then(|mut i| i.checked.take()) else { return };
        self.settings.update_checked = now();
        self.settings.save();
        let Some(m) = found else { return };
        let hidden = unsafe { !IsWindowVisible(self.host.hwnd).as_bool() };
        if hidden && self.host.tray_icon {
            let title = format!("Goby {}", update::show(m.version));
            tray::notify(self.host.hwnd, &title, tr!("update.notify"));
        }
        self.update = Some(update::show(m.version));
        self.host.manifest = Some(m);
        self.dirty = true;
    }

    /// Starts the verified download as the installer. It closes this copy, puts itself in
    /// its place and starts it again; the file stays locked against writes until then.
    pub(super) fn update_ready(&mut self) {
        let Some(done) = self.host.inbox.lock().ok().and_then(|mut i| i.downloaded.take()) else { return };
        let Some((path, lock)) = done else {
            self.updating = Updating::Failed;
            self.dirty = true;
            return;
        };
        let exe = HSTRING::from(path.as_str());
        install::launch(&exe, "--install --run", None, SW_SHOWNORMAL);
        drop(lock);
    }
}

pub(super) fn inbox() -> Arc<Mutex<Inbox>> {
    Arc::new(Mutex::new(Inbox::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_waits_a_day_and_is_due_again_when_the_clock_went_back() {
        let last = 1_800_000_000;
        assert!(!due(last, last));
        assert!(!due(last + EVERY - 1, last));
        assert!(due(last + EVERY, last));
        assert!(due(last - 1, last));
        assert!(due(last - 365 * 24 * 60 * 60, last));
        assert!(due(0, u64::MAX));
        assert!(!due(u64::MAX, u64::MAX - EVERY + 1));
    }
}
