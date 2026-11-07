//! Updates in the window: a daily check in the background, the card that offers a newer
//! release, and handing its verified download to the installer. The checking itself, and
//! everything that makes a download trustworthy, is in `crate::update`.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use super::*;
use crate::update::{self, Manifest};

/// Sent by the background threads when a check or a download has finished.
pub(super) const WM_UPDATE_CHECKED: u32 = WM_APP + 3;
pub(super) const WM_UPDATE_READY: u32 = WM_APP + 4;

/// A day, give or take the hour a computer happens to start at.
const EVERY: u64 = 20 * 60 * 60;

#[derive(Default)]
pub(super) struct Inbox {
    checked: Option<Option<Manifest>>,
    downloaded: Option<Option<(PathBuf, File)>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Updating {
    Idle,
    Downloading,
    Failed,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Runs `work` on a thread of its own and wakes the window with `msg` when it is done.
fn in_background(hwnd: HWND, msg: u32, work: impl FnOnce() + Send + 'static) {
    let raw = hwnd.0 as isize;
    std::thread::spawn(move || {
        work();
        unsafe {
            let _ = PostMessageW(Some(HWND(raw as _)), msg, WPARAM(0), LPARAM(0));
        }
    });
}

impl App {
    /// Starts the daily check, a little after start-up so it never competes with the audio.
    pub(super) fn check_for_update(&mut self, delay: Duration) {
        if !self.settings.updates || now().saturating_sub(self.settings.update_checked) < EVERY {
            return;
        }
        let inbox = self.inbox.clone();
        in_background(self.hwnd, WM_UPDATE_CHECKED, move || {
            std::thread::sleep(delay);
            let found = update::check();
            if let Ok(mut inbox) = inbox.lock() {
                inbox.checked = Some(found);
            }
        });
    }

    pub(super) fn update_checked(&mut self) {
        let Some(found) = self.inbox.lock().ok().and_then(|mut i| i.checked.take()) else { return };
        self.settings.update_checked = now();
        self.settings.save();
        let Some(m) = found else { return };
        let hidden = unsafe { !IsWindowVisible(self.hwnd).as_bool() };
        if hidden && self.tray_icon {
            let title = format!("Goby {}", update::show(m.version));
            tray::notify(
                self.hwnd,
                &title,
                self.t(
                    "Доступна новая версия. Откройте Goby, чтобы обновить.",
                    "A new version is out. Open Goby to update.",
                ),
            );
        }
        self.update = Some(m);
        self.dirty = true;
    }

    /// The installed copy downloads and installs the update; a portable one, which Goby
    /// does not manage, is sent to the release page instead.
    pub(super) fn install_update(&mut self) {
        let Some(m) = self.update.clone() else { return };
        if !install::is_installed_copy() {
            install::launch(w!("https://github.com/Sadykhzadeh/goby/releases/latest"), "", None, SW_SHOWNORMAL);
            return;
        }
        if self.updating == Updating::Downloading {
            return;
        }
        self.updating = Updating::Downloading;
        self.dirty = true;
        let inbox = self.inbox.clone();
        in_background(self.hwnd, WM_UPDATE_READY, move || {
            let file = update::download(&m);
            if let Ok(mut inbox) = inbox.lock() {
                inbox.downloaded = Some(file);
            }
        });
    }

    /// Starts the verified download as the installer. It closes this copy, puts itself in
    /// its place and starts it again; the file stays locked against writes until then.
    pub(super) fn update_ready(&mut self) {
        let Some(done) = self.inbox.lock().ok().and_then(|mut i| i.downloaded.take()) else { return };
        let Some((path, lock)) = done else {
            self.updating = Updating::Failed;
            self.dirty = true;
            return;
        };
        let exe = HSTRING::from(path.as_os_str());
        install::launch(&exe, "--install --run", None, SW_SHOWNORMAL);
        drop(lock);
    }

    pub(super) fn update_card(&mut self, g: &Gfx, y: &mut f32) -> Result<()> {
        let Some(m) = &self.update else { return Ok(()) };
        let title = format!("Goby {} {}", update::show(m.version), self.t("уже вышел", "is out"));
        let desc = match (self.updating, install::is_installed_copy()) {
            (Updating::Downloading, _) => {
                self.t("Скачиваю и проверяю подпись…", "Downloading and checking the signature…")
            }
            (Updating::Failed, _) => self.t(
                "Не удалось скачать файл с верной подписью. Попробуйте позже",
                "Could not download a file that matches the signature. Try again later",
            ),
            (_, true) => self.t(
                "Установить: файл проверяется по подписи автора",
                "Install: the file is checked against the author's signature",
            ),
            (_, false) => self.t("Скачать со страницы релизов", "Download it from the releases page"),
        };
        let hit = if self.updating == Updating::Downloading { Hit::None } else { Hit::Update };
        self.row_card(g, y, hit, glyph::UPDATE, &title, desc)?;
        *y += 20.0;
        Ok(())
    }
}

pub(super) fn inbox() -> Arc<Mutex<Inbox>> {
    Arc::new(Mutex::new(Inbox::default()))
}
