//! Living in the tray: the notification icon, its menu, closing to it, and the hotkeys.

use super::*;

impl App {
    /// What the icon says on hover: the voice that is on, or that the effect is off.
    pub(super) fn tip(&self) -> String {
        let voice = voices::ALL.get(self.voice).map_or("", |v| v.name(self.ru));
        match self.settings.enabled {
            true => format!("Goby: {voice}"),
            false => format!("Goby: {}", self.t("эффект выключен", "effect off")),
        }
    }

    pub(super) fn add_tray_icon(&mut self) {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, self.dpi as u32) };
        let Some(icon) = make_icon(size, icon::RGB) else { return };
        self.tray_tip = self.tip();
        self.tray_icon = tray::add(self.hwnd, icon, &self.tray_tip);
    }

    pub(super) fn show(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if IsIconic(self.hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
            let _ = SetForegroundWindow(self.hwnd);
        }
        self.dirty = true;
    }

    /// Closing the window: it goes to the tray while the icon is there to bring it back.
    /// True when it should be destroyed instead, which frees `self`, so the caller does it.
    pub(super) fn close(&mut self) -> bool {
        if !self.settings.tray || !self.tray_icon {
            return true;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.gfx = None;
        if !self.settings.tray_told {
            self.settings.tray_told = true;
            self.settings.save();
            tray::notify(
                self.hwnd,
                self.t("Goby работает в трее", "Goby is still running"),
                self.t(
                    "Голос остаётся включённым. Щёлкните значок, чтобы открыть окно, или правой кнопкой, чтобы выйти.",
                    "Your voice stays on. Click the icon to open the window, or right-click it to quit.",
                ),
            );
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
        let mut menu = vec![item(self.t("Открыть Goby", "Open Goby"), false, TrayAct::Open), line()];
        let favorites = self.favorites();
        let current = self.current();
        for &f in favorites.iter().take(12) {
            let on = f == current && self.settings.enabled;
            menu.push(item(&self.label(f), on, TrayAct::Pick(f)));
        }
        if !favorites.is_empty() {
            menu.push(line());
        }
        let voice = voices::ALL.get(self.voice).map_or("", |v| v.name(self.ru));
        let effect = format!("{}: {voice}", self.t("Эффект", "Effect"));
        menu.push(item(&effect, self.settings.enabled, TrayAct::Effect));
        menu.push(line());
        menu.push(item(self.t("Выйти", "Quit"), false, TrayAct::Quit));
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
                self.settings.save();
                self.dirty = true;
            }
            TrayAct::Quit => return true,
            TrayAct::None => {}
        }
        false
    }

    pub(super) fn use_hotkeys(&mut self) {
        hotkeys::unregister(self.hwnd);
        self.hotkeys_ok = !self.settings.hotkeys || hotkeys::register(self.hwnd);
    }

    pub(super) fn hotkey(&mut self, action: hotkeys::Action) {
        let step = match action {
            hotkeys::Action::Toggle => {
                self.settings.enabled = !self.settings.enabled;
                self.apply();
                self.settings.save();
                self.dirty = true;
                return;
            }
            hotkeys::Action::Next => 1,
            hotkeys::Action::Previous => -1,
        };
        // Favourites when there are any, otherwise every voice but Off, which the toggle covers.
        let mut list = self.favorites();
        if list.is_empty() {
            list = (1..voices::ALL.len()).map(Fav::Voice).collect();
        }
        if let Some(f) = hotkeys::step_in(&list, Some(self.current()), step) {
            self.choose(f, true);
        }
    }
}
