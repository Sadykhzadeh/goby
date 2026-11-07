//! Mouse, keyboard and the menus a click opens.

use super::*;

impl App {
    /// The "Add a program" menu: recent microphone users, plus "Other program…".
    pub(super) fn app_popup(&self) -> Option<Popup> {
        let row = self.hits.iter().find(|(_, h)| *h == Hit::AddApp).map(|(r, _)| *r)?;
        let labels = self
            .mic_apps
            .iter()
            .map(|a| {
                if a.active {
                    format!("{}\t{}", a.name, self.t("слушает сейчас", "listening now"))
                } else {
                    a.name.clone()
                }
            })
            .collect();
        Some(Popup {
            pick: None,
            labels,
            keys: self.mic_apps.iter().map(|a| a.exe.clone()).collect(),
            checked: None,
            browse: Some((
                self.t("Другая программа…", "Other program…").into(),
                self.t("Какая программа должна слышать Goby?", "Which program should hear Goby?").into(),
            )),
            at: self.screen_point(row.l + 52.0, row.b),
        })
    }

    /// The device menu under a picker card, keyed by endpoint id.
    pub(super) fn device_popup(&mut self, pick: Pick) -> Option<Popup> {
        self.refresh_devices();
        let card = self.hits.iter().find(|(_, h)| *h == Hit::Pick(pick)).map(|(r, _)| *r)?;
        let (items, selected) = match pick {
            Pick::Mic => (&self.inputs, self.input),
            Pick::Out => (&self.outputs, self.output),
        };
        Some(Popup {
            pick: Some(pick),
            labels: items.iter().map(|e| e.name.clone()).collect(),
            keys: items.iter().map(|e| e.id.clone()).collect(),
            checked: Some(selected),
            browse: None,
            at: self.screen_point(card.l + 52.0, card.b),
        })
    }

    pub(super) fn screen_point(&self, x: f32, y: f32) -> POINT {
        let scale = self.dpi / 96.0;
        let mut p = POINT { x: (x * scale) as i32, y: (y * scale) as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut p);
        }
        p
    }

    /// Applies what was chosen in a popup, by key: the lists may have changed meanwhile.
    pub(super) fn popup_done(&mut self, pick: Option<Pick>, key: String) {
        match pick {
            None => self.route(key),
            Some(pick) => {
                let list = if pick == Pick::Mic { &self.inputs } else { &self.outputs };
                if let Some(i) = list.iter().position(|e| e.id == key) {
                    match pick {
                        Pick::Mic => (self.input, self.settings.input) = (i, key),
                        Pick::Out => (self.output, self.settings.output) = (i, key),
                    }
                    self.restart();
                }
            }
        }
        self.dirty = true;
    }

    pub(super) fn hit_at(&self, p: (f32, f32)) -> Hit {
        self.hits.iter().rev().find(|(r, _)| r.has(p)).map_or(Hit::None, |(_, h)| *h)
    }

    pub(super) fn slider_from_x(&mut self, k: Knob, x: f32) {
        let Some((_, track)) = self.tracks.iter().find(|(kk, _)| *kk == k) else { return };
        let (lo, hi, _, _) = k.range();
        let t = ((x - track.l) / track.w()).clamp(0.0, 1.0);
        self.set_knob(k, lo + t * (hi - lo));
    }

    /// Handles a click. Menus are returned rather than shown: see `Popup`.
    pub(super) fn press(&mut self, double: bool) -> Option<Popup> {
        let hit = self.hit_at(self.mouse);
        match hit {
            Hit::Tile(i) => self.choose(Fav::Voice(i), false),
            Hit::Preset(p) => self.choose(Fav::Preset(p), false),
            Hit::DropPreset(p) => self.drop_preset(p),
            Hit::SavePreset => self.save_preset(),
            Hit::Update => self.install_update(),
            Hit::Advanced => {
                self.advanced = !self.advanced;
                self.dirty = true;
            }
            Hit::Star(i) => self.toggle_favorite(i),
            Hit::Tab(c) => {
                self.shelf = c;
                self.gallery_scroll.target = 0.0;
                self.dirty = true;
            }
            Hit::Slider(k) if double => {
                self.set_knob(k, k.range().3);
                self.settings.save();
            }
            Hit::Slider(k) => {
                self.drag = Some(Drag { hit, from: self.mouse });
                unsafe { SetCapture(self.hwnd) };
                self.slider_from_x(k, self.mouse.0);
            }
            Hit::Toggle(s) => {
                match s {
                    Switch::Effect => self.settings.enabled = !self.settings.enabled,
                    Switch::Gate => self.settings.gate = !self.settings.gate,
                    Switch::Monitor => self.settings.monitor = !self.settings.monitor,
                    Switch::Exclusive => self.settings.exclusive = !self.settings.exclusive,
                    Switch::Tray => self.settings.tray = !self.settings.tray,
                    Switch::Autostart => {
                        install::set_start_with_windows(!self.autostart);
                        self.autostart = install::starts_with_windows();
                    }
                    Switch::Updates => {
                        self.settings.updates = !self.settings.updates;
                        if self.settings.updates {
                            self.settings.update_checked = 0;
                            self.check_for_update(Duration::ZERO);
                        }
                    }
                    Switch::Hotkeys => {
                        self.settings.hotkeys = !self.settings.hotkeys;
                        self.use_hotkeys();
                    }
                }
                self.apply();
                if matches!(s, Switch::Monitor | Switch::Exclusive) {
                    self.restart();
                }
                self.settings.save();
            }
            Hit::Pick(p) => return self.device_popup(p),
            Hit::Remove(i) => {
                if i < self.settings.routed.len() {
                    let exe = self.settings.routed.remove(i);
                    self.settings.unroute.push(exe);
                    self.settings.save();
                    self.pinned.clear();
                    self.sync_apps();
                }
            }
            Hit::AddApp => return self.app_popup(),
            Hit::About => unsafe {
                ShellExecuteW(
                    Some(self.hwnd),
                    w!("open"),
                    w!("https://azer.one"),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            },
            Hit::Scrollbar(pane) => {
                let at = match pane {
                    Pane::Gallery => self.gallery_scroll.value,
                    Pane::Settings => self.settings_scroll.value,
                };
                self.drag = Some(Drag { hit, from: (self.mouse.1, at) });
                unsafe { SetCapture(self.hwnd) };
            }
            Hit::Nav(page) => self.go(page),
            Hit::Offer(i) => {
                if let Some(app) = self.mic_apps.get(i) {
                    self.route(app.exe.clone());
                }
            }
            Hit::Downloads => install::launch(w!("explorer.exe"), "shell:Downloads", None, SW_SHOWNORMAL),
            Hit::CableSite => unsafe {
                self.cable_opened = true;
                ShellExecuteW(
                    Some(self.hwnd),
                    w!("open"),
                    w!("https://vb-audio.com/Cable/"),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    SW_SHOWNORMAL,
                );
            },
            Hit::None => {}
        }
        None
    }

    pub(super) fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.settings_scroll = Anim::default();
            // The page rises into place the way WinUI's do, unless animations are off
            // in Windows' Accessibility settings.
            if animations_on() {
                self.page_in = Anim { value: 0.0, target: 1.0 };
            }
            self.hits.clear();
            self.hover = Hit::None;
        }
        self.dirty = true;
    }

    pub(super) fn moved(&mut self) {
        match self.drag.as_ref().map(|d| (d.hit, d.from)) {
            Some((Hit::Slider(k), _)) => self.slider_from_x(k, self.mouse.0),
            Some((Hit::Scrollbar(pane), (y0, s0))) => {
                let (view, max) = match pane {
                    Pane::Gallery => (self.lay.gallery.h(), self.gallery_max),
                    Pane::Settings => (self.lay.pane.h(), self.settings_max),
                };
                let content = view + max;
                let to = (s0 + (self.mouse.1 - y0) * content / view).clamp(0.0, max);
                let a = match pane {
                    Pane::Gallery => &mut self.gallery_scroll,
                    Pane::Settings => &mut self.settings_scroll,
                };
                *a = Anim { value: to, target: to };
                self.dirty = true;
            }
            _ => {}
        }
        let hover = self.hit_at(self.mouse);
        if hover != self.hover {
            self.hover = hover;
            self.dirty = true;
        }
    }

    pub(super) fn release(&mut self) {
        if self.drag.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
            self.settings.save();
            self.dirty = true;
        }
    }

    pub(super) fn wheel(&mut self, notches: f32) {
        if self.page != Page::Voices {
            let a = &mut self.settings_scroll;
            a.target = (a.target - notches * 72.0).clamp(0.0, self.settings_max);
        } else if self.lay.tabs.has(self.mouse) {
            self.tab_scroll = (self.tab_scroll - notches * 80.0).clamp(0.0, self.tab_max);
        } else if self.lay.gallery.has(self.mouse) {
            let a = &mut self.gallery_scroll;
            a.target = (a.target - notches * (TILE_H + TILE_GAP)).clamp(0.0, self.gallery_max);
        }
        self.dirty = true;
    }

    /// Arrow keys walk the gallery like a native grid view.
    pub(super) fn key(&mut self, vk: u16) -> bool {
        if self.page != Page::Voices {
            // Esc and Backspace go back, as in Windows Settings.
            if matches!(VIRTUAL_KEY(vk), VK_ESCAPE | VK_BACK) {
                self.go(Page::Voices);
                return true;
            }
            return false;
        }
        let list = self.filtered();
        if list.is_empty() {
            return false;
        }
        let pos = list.iter().position(|f| self.is_chosen(*f)).unwrap_or(0);
        let next = match VIRTUAL_KEY(vk) {
            VK_LEFT => pos.saturating_sub(1),
            VK_RIGHT => (pos + 1).min(list.len() - 1),
            VK_UP => pos.saturating_sub(self.lay.cols),
            VK_DOWN => (pos + self.lay.cols).min(list.len() - 1),
            VK_HOME => 0,
            VK_END => list.len() - 1,
            _ => return false,
        };
        self.choose(list[next], false);
        // Keep the selection in view.
        let row = (next / self.lay.cols) as f32 * (TILE_H + TILE_GAP);
        let a = &mut self.gallery_scroll;
        if row < a.target {
            a.target = row;
        } else if row + TILE_H > a.target + self.lay.gallery.h() {
            a.target = (row + TILE_H - self.lay.gallery.h()).min(self.gallery_max);
        }
        true
    }
}
