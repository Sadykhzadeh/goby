//! Pointer, keys, hotkeys and the menus a click opens. Both windows feed their events in
//! here, in DIPs.

use super::*;

impl<H: Host> App<H> {
    /// The "Add a program" menu: recent microphone users, plus "Other program…".
    fn app_popup(&self) -> Option<Popup> {
        let row = self.hits.iter().find(|(_, h)| *h == Hit::AddApp).map(|(r, _)| *r)?;
        let labels = self
            .mic_apps
            .iter()
            .map(|a| if a.active { format!("{}\t{}", a.name, tr!("programs.menu.active")) } else { a.name.clone() })
            .collect();
        Some(Popup {
            pick: None,
            labels,
            keys: self.mic_apps.iter().map(|a| a.exe.clone()).collect(),
            checked: None,
            browse: Some((tr!("programs.menu.other").into(), tr!("programs.menu.title").into())),
            at: (row.l + 52.0, row.b),
        })
    }

    /// The device menu under a picker card, keyed by endpoint id.
    fn device_popup(&mut self, pick: Pick) -> Option<Popup> {
        self.refresh_devices();
        let card = self.hits.iter().find(|(_, h)| *h == Hit::Pick(pick)).map(|(r, _)| *r)?;
        let (items, selected) =
            if pick == Pick::Mic { (&self.inputs, self.input) } else { (&self.outputs, self.output) };
        Some(Popup {
            pick: Some(pick),
            labels: items.iter().map(|e| e.name.clone()).collect(),
            keys: items.iter().map(|e| e.id.clone()).collect(),
            checked: Some(selected),
            browse: None,
            at: (card.l + 52.0, card.b),
        })
    }

    /// What the language menu calls a choice from settings.ini: a language by its own
    /// name, and "auto" with the one it stands for now.
    pub(super) fn language_name(&self, choice: &str) -> String {
        let name = lang::LANGUAGES[lang::resolve(choice, &self.host.language())].1;
        match choice {
            "auto" => trf!("settings.language.auto", &[("language", name)]),
            _ => name.to_string(),
        }
    }

    /// The language menu: Auto, then every language.
    fn language_popup(&self) -> Option<Popup> {
        let card = self.hits.iter().find(|(_, h)| *h == Hit::Pick(Pick::Lang)).map(|(r, _)| *r)?;
        let keys: Vec<String> =
            std::iter::once("auto").chain(lang::LANGUAGES.iter().map(|l| l.0)).map(String::from).collect();
        Some(Popup {
            pick: Some(Pick::Lang),
            labels: keys.iter().map(|k| self.language_name(k)).collect(),
            checked: keys.iter().position(|k| *k == self.settings.lang),
            keys,
            browse: None,
            at: (card.l + 52.0, card.b),
        })
    }

    /// Shows everything in the language `choice` from now on, and keeps it.
    fn set_language(&mut self, choice: String) {
        self.settings.lang = choice;
        lang::choose(&self.settings.lang, &self.host.language());
        // The tabs are measured once per language, and the marker under them follows.
        self.tab_widths.clear();
        self.tab_w = Anim::default();
        self.save();
        self.dirty = true;
    }

    /// A menu from `down` has closed, with the key of what was chosen, if anything.
    pub fn menu_done(&mut self, pick: Option<Pick>, chosen: Option<String>) {
        if let Some(key) = chosen {
            self.popup_done(pick, key);
        }
        self.hover = self.hit_at(self.mouse);
        self.dirty = true;
    }

    /// Applies what was chosen in a popup, by key: the lists may have changed meanwhile.
    fn popup_done(&mut self, pick: Option<Pick>, key: String) {
        match pick {
            None => self.route(key),
            Some(Pick::Lang) => self.set_language(key),
            Some(pick) => {
                let list = if pick == Pick::Mic { &self.inputs } else { &self.outputs };
                if let Some(i) = list.iter().position(|e| e.id == key) {
                    match pick {
                        Pick::Mic => (self.input, self.settings.input) = (i, key),
                        _ => (self.output, self.settings.output) = (i, key),
                    }
                    self.restart();
                }
            }
        }
        self.dirty = true;
    }

    fn hit_at(&self, p: (f32, f32)) -> Hit {
        self.hits.iter().rev().find(|(r, _)| r.has(p)).map_or(Hit::None, |(_, h)| *h)
    }

    // Touch and the website's own buttons; the app has neither.
    #[cfg_attr(windows, allow(dead_code))]
    /// Something at `p` follows a press-and-drag: a slider or a scrollbar.
    pub fn draggable_at(&self, p: (f32, f32)) -> bool {
        matches!(self.hit_at(p), Hit::Slider(_) | Hit::Scrollbar(_))
    }

    fn slider_from_x(&mut self, k: Knob, x: f32) {
        let Some((_, track)) = self.tracks.iter().find(|(kk, _)| *kk == k) else { return };
        let (lo, hi, _, _) = k.range();
        let t = ((x - track.l) / track.w()).clamp(0.0, 1.0);
        self.set_knob(k, lo + t * (hi - lo));
    }

    /// A press of the main button at `p`. A menu it opens comes back to be shown, and its
    /// answer goes to `menu_done`.
    pub fn down(&mut self, p: (f32, f32), double: bool) -> Option<Popup> {
        self.mouse = p;
        let popup = self.press(double);
        if popup.is_none() {
            self.hover = self.hit_at(self.mouse);
            self.dirty = true;
        }
        popup
    }

    fn press(&mut self, double: bool) -> Option<Popup> {
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
                self.save();
            }
            Hit::Slider(k) => {
                self.drag = Some(Drag { hit, from: self.mouse });
                self.host.capture(true);
                self.slider_from_x(k, self.mouse.0);
            }
            Hit::Toggle(s) => self.flip(s),
            Hit::Pick(Pick::Lang) => return self.language_popup(),
            Hit::Pick(p) => return self.device_popup(p),
            Hit::Remove(i) => {
                if i < self.settings.routed.len() {
                    let exe = self.settings.routed.remove(i);
                    self.settings.unroute.push(exe);
                    self.save();
                    self.host.repin();
                    self.sync_apps();
                }
            }
            Hit::AddApp => return self.app_popup(),
            Hit::About => self.host.open(Link::Author),
            Hit::Scrollbar(pane) => {
                let at = match pane {
                    Pane::Gallery => self.gallery_scroll.value,
                    Pane::Settings => self.settings_scroll.value,
                };
                self.drag = Some(Drag { hit, from: (self.mouse.1, at) });
                self.host.capture(true);
            }
            Hit::Nav(page) => self.go(page),
            Hit::Offer(i) => {
                if let Some(app) = self.mic_apps.get(i) {
                    self.route(app.exe.clone());
                }
            }
            Hit::Downloads => self.host.open(Link::Downloads),
            Hit::CableSite => {
                self.cable_opened = true;
                self.host.open(Link::Cable);
            }
            Hit::None => {}
        }
        None
    }

    /// Turns a switch over, as a click on it does.
    pub fn flip(&mut self, s: Switch) {
        match s {
            Switch::Effect => self.settings.enabled = !self.settings.enabled,
            Switch::Gate => self.settings.gate = !self.settings.gate,
            Switch::Monitor => self.settings.monitor = !self.settings.monitor,
            Switch::Exclusive => self.settings.exclusive = !self.settings.exclusive,
            Switch::Tray => self.settings.tray = !self.settings.tray,
            Switch::Autostart => {
                self.host.set_autostart(!self.autostart);
                self.autostart = self.host.autostart();
            }
            Switch::Updates => {
                self.settings.updates = !self.settings.updates;
                if self.settings.updates {
                    self.settings.update_checked = 0;
                    self.host.check_for_update(&self.settings, Duration::ZERO);
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
        self.save();
    }

    // Touch and the website's own buttons; the app has neither.
    #[cfg_attr(windows, allow(dead_code))]
    /// Whether a switch is on.
    pub fn is_on(&self, s: Switch) -> bool {
        let st = &self.settings;
        match s {
            Switch::Effect => st.enabled,
            Switch::Gate => st.gate,
            Switch::Monitor => st.monitor,
            Switch::Exclusive => st.exclusive,
            Switch::Tray => st.tray,
            Switch::Autostart => self.autostart,
            Switch::Hotkeys => st.hotkeys,
            Switch::Updates => st.updates,
        }
    }

    pub fn use_hotkeys(&mut self) {
        self.hotkeys_ok = self.host.hotkeys(self.settings.hotkeys);
    }

    /// The installed copy downloads and installs the update; a portable one, which Goby
    /// does not manage, is sent to the release page instead.
    fn install_update(&mut self) {
        if self.update.is_none() {
            return;
        }
        if self.host.installed() {
            if self.updating == Updating::Downloading {
                return;
            }
            self.updating = Updating::Downloading;
            self.dirty = true;
        }
        self.host.update();
    }

    fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.settings_scroll = Anim::default();
            // The page rises into place the way WinUI's do, unless animations are off
            // in Windows' Accessibility settings.
            if self.host.animations() {
                self.page_in = Anim { value: 0.0, target: 1.0 };
            }
            self.hits.clear();
            self.hover = Hit::None;
        }
        self.dirty = true;
    }

    /// The pointer moved to `p`.
    pub fn moved(&mut self, p: (f32, f32)) {
        self.mouse = p;
        match self.drag.as_ref().map(|d| (d.hit, d.from)) {
            Some((Hit::Slider(k), _)) => self.slider_from_x(k, self.mouse.0),
            Some((Hit::Scrollbar(pane), (y0, s0))) => {
                let (view, max) = match pane {
                    Pane::Gallery => (self.lay.gallery.h(), self.gallery_max),
                    Pane::Settings => (self.lay.pane.h(), self.settings_max),
                };
                let content = view + max;
                let to = clamp(s0 + (self.mouse.1 - y0) * content / view, 0.0, max);
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

    /// The pointer left the window.
    pub fn left(&mut self) {
        self.hover = Hit::None;
        self.dirty = true;
    }

    /// The main button came up.
    pub fn up(&mut self) {
        if self.drag.take().is_some() {
            self.host.capture(false);
            self.save();
            self.dirty = true;
        }
    }

    /// A right click stars or unstars the voice under it.
    pub fn right_click(&mut self, p: (f32, f32)) {
        self.mouse = p;
        if let Hit::Tile(i) | Hit::Star(i) = self.hit_at(self.mouse) {
            self.toggle_favorite(i);
        }
    }

    /// The wheel turned by `notches` (positive away from the user) with the pointer at `p`.
    pub fn wheel(&mut self, p: (f32, f32), notches: f32) {
        self.mouse = p;
        if self.page != Page::Voices {
            let a = &mut self.settings_scroll;
            a.target = clamp(a.target - notches * 72.0, 0.0, self.settings_max);
        } else if self.lay.tabs.has(self.mouse) {
            self.tab_scroll = clamp(self.tab_scroll - notches * 80.0, 0.0, self.tab_max);
        } else if self.lay.gallery.has(self.mouse) {
            let a = &mut self.gallery_scroll;
            a.target = clamp(a.target - notches * (TILE_H + TILE_GAP), 0.0, self.gallery_max);
        }
        self.dirty = true;
    }

    // Touch and the website's own buttons; the app has neither.
    #[cfg_attr(windows, allow(dead_code))]
    /// A finger dragged the pane under `p` by `dy` DIPs. False when there is no pane there or
    /// it is already at that end, so the page around the window can scroll instead.
    pub fn drag_scroll(&mut self, p: (f32, f32), dy: f32) -> bool {
        let (a, max) = if self.page != Page::Voices && self.lay.pane.has(p) {
            (&mut self.settings_scroll, self.settings_max)
        } else if self.page == Page::Voices && self.lay.gallery.has(p) {
            (&mut self.gallery_scroll, self.gallery_max)
        } else {
            return false;
        };
        let to = clamp(a.target - dy, 0.0, max);
        if to == a.target {
            return false;
        }
        *a = Anim { value: to, target: to };
        self.dirty = true;
        true
    }

    /// Arrow keys walk the gallery like a native grid view.
    pub fn key(&mut self, key: Key) -> bool {
        if self.page != Page::Voices {
            // Esc and Backspace go back, as in Windows Settings.
            if matches!(key, Key::Escape | Key::Back) {
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
        let next = match key {
            Key::Left => pos.saturating_sub(1),
            Key::Right => (pos + 1).min(list.len() - 1),
            Key::Up => pos.saturating_sub(self.lay.cols),
            Key::Down => (pos + self.lay.cols).min(list.len() - 1),
            Key::Home => 0,
            Key::End => list.len() - 1,
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

    pub fn hotkey(&mut self, action: Hotkey) {
        let step = match action {
            Hotkey::Toggle => {
                self.settings.enabled = !self.settings.enabled;
                self.apply();
                self.save();
                self.dirty = true;
                return;
            }
            Hotkey::Next => 1,
            Hotkey::Previous => -1,
        };
        // Favourites when there are any, otherwise every voice but Off, which the toggle covers.
        let mut list = self.favorites();
        if list.is_empty() {
            list = (1..voices::ALL.len()).map(Fav::Voice).collect();
        }
        if let Some(f) = step_in(&list, Some(self.current()), step) {
            self.choose(f, true);
        }
    }
}

/// The entry `step` places away from `current` in `list`, wrapping around. From outside the
/// list a step forward lands on its first entry and a step back on its last.
fn step_in<T: Copy + PartialEq>(list: &[T], current: Option<T>, step: isize) -> Option<T> {
    if list.is_empty() {
        return None;
    }
    let len = list.len() as isize;
    let at = match current.and_then(|c| list.iter().position(|x| *x == c)) {
        Some(pos) => (pos as isize + step).rem_euclid(len),
        None if step > 0 => 0,
        None => len - 1,
    };
    Some(list[at as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_around_and_enters_from_either_end() {
        let list = [1, 2, 3, 4];
        assert_eq!(step_in(&list, Some(1), 1), Some(2));
        assert_eq!(step_in(&list, Some(4), 1), Some(1));
        assert_eq!(step_in(&list, Some(1), -1), Some(4));
        assert_eq!(step_in(&list, Some(0), 1), Some(1));
        assert_eq!(step_in(&list, Some(0), -1), Some(4));
        assert_eq!(step_in(&list, None, 1), Some(1));
        assert_eq!(step_in::<usize>(&[], Some(1), 1), None);
    }
}
