//! Living in the tray: the notification icon, its menu, and closing to it.

use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::WindowsAndMessaging::{
    IsIconic, SHOW_WINDOW_CMD, SM_CXSMICON, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWMAXIMIZED, SetForegroundWindow,
    ShowWindow,
};

use super::{Win, make_icon};
use crate::lang::{tr, trf};
use crate::ui::Fav;
use crate::{icon, tray, voices};

/// What a line of the tray icon's menu does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TrayAct {
    None,
    Open,
    Pick(Fav),
    Effect,
    Quit,
}

impl Win {
    /// What the icon says on hover: the voice that is on, or that the effect is off.
    pub(super) fn tip(&self) -> String {
        let voice = voices::ALL.get(self.voice).map_or("", |v| v.name());
        match self.settings.enabled {
            true => format!("Goby: {voice}"),
            false => tr!("tray.off").into(),
        }
    }

    pub(super) fn add_tray_icon(&mut self) {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, self.dpi as u32) };
        let Some(icon) = make_icon(size, icon::RGB) else { return };
        self.host.tray_tip = self.tip();
        self.host.tray_icon = tray::add(self.host.hwnd, icon, &self.host.tray_tip);
    }

    /// Keeps the icon's tip on the voice that is playing.
    pub(super) fn update_tip(&mut self) {
        if self.host.tray_icon {
            let tip = self.tip();
            if tip != self.host.tray_tip {
                tray::set_tip(self.host.hwnd, &tip);
                self.host.tray_tip = tip;
            }
        }
    }

    /// How to show the window: maximised the first time when it was closed that way.
    pub(super) fn show_command(&mut self) -> SHOW_WINDOW_CMD {
        if std::mem::take(&mut self.host.start_maximized) { SW_SHOWMAXIMIZED } else { SW_SHOW }
    }

    pub(super) fn show(&mut self) {
        unsafe {
            let hwnd = self.host.hwnd;
            let cmd = if IsIconic(hwnd).as_bool() { SW_RESTORE } else { self.show_command() };
            let _ = ShowWindow(hwnd, cmd);
            let _ = SetForegroundWindow(hwnd);
        }
        self.dirty = true;
    }

    /// Closing the window: it goes to the tray while the icon is there to bring it back.
    /// True when it should be destroyed instead, which frees `self`, so the caller does it.
    pub(super) fn close(&mut self) -> bool {
        if !self.settings.tray || !self.host.tray_icon {
            return true;
        }
        self.remember_window();
        unsafe {
            let _ = ShowWindow(self.host.hwnd, SW_HIDE);
        }
        self.host.gfx = None;
        let told = std::mem::replace(&mut self.settings.tray_told, true);
        self.save();
        if !told {
            tray::notify(self.host.hwnd, tr!("tray.told"), tr!("tray.told.desc"));
        }
        false
    }

    /// The icon's menu: open the window, switch to a favourite, the effect switch, quit.
    pub(super) fn tray_menu(&self) -> Vec<(tray::Item, TrayAct)> {
        let item = |label: &str, checked: bool, act: TrayAct| {
            let default = act == TrayAct::Open;
            (tray::Item { label: Some(label.to_string()), checked, default }, act)
        };
        let line = || (tray::Item { label: None, checked: false, default: false }, TrayAct::None);
        let mut menu = vec![item(tr!("tray.open"), false, TrayAct::Open), line()];
        let favorites = self.favorites();
        let current = self.current();
        for &f in favorites.iter().take(12) {
            let on = f == current && self.settings.enabled;
            menu.push(item(&self.label(f), on, TrayAct::Pick(f)));
        }
        if !favorites.is_empty() {
            menu.push(line());
        }
        let voice = voices::ALL.get(self.voice).map_or("", |v| v.name());
        let effect = trf!("tray.effect", &[("voice", voice)]);
        menu.push(item(&effect, self.settings.enabled, TrayAct::Effect));
        menu.push(line());
        menu.push(item(tr!("tray.quit"), false, TrayAct::Quit));
        menu
    }

    /// Acts on a menu item; true for Quit, which the caller carries out like `close`.
    pub(super) fn tray_act(&mut self, act: TrayAct) -> bool {
        match act {
            TrayAct::Open => self.show(),
            TrayAct::Pick(f) => self.choose(f, true),
            TrayAct::Effect => {
                self.settings.enabled = !self.settings.enabled;
                self.apply();
                self.save();
                self.dirty = true;
            }
            TrayAct::Quit => return true,
            TrayAct::None => {}
        }
        false
    }
}
