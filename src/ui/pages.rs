//! The three pages and the navigation rail, drawn each frame from the current state.

use super::*;
use controls::Icon;

impl<H: Host> App<H> {
    /// Draws the whole window from the current state, and notes what can be clicked.
    pub fn draw(&mut self, g: &dyn Canvas) {
        self.hits.clear();
        self.tracks.clear();
        self.draw_rail(g);
        let entering = self.page_in.value < 1.0;
        if entering {
            let k = self.page_in.value;
            g.shift((1.0 - k) * 28.0, k);
        }
        match self.page {
            Page::Voices => {
                self.draw_scope(g);
                self.draw_quick(g);
                self.draw_tabs(g);
                self.draw_gallery(g)
            }
            Page::Programs => self.draw_programs(g),
            Page::Settings => self.draw_settings(g),
        }
        if entering {
            g.unshift();
        }
        self.draw_rail_tip(g);
    }

    pub(super) fn rail(&self) -> [(Page, R, u16, &'static str); 3] {
        [
            (Page::Voices, self.lay.nav_voices, glyph::VOICES, tr("page.voices")),
            (Page::Programs, self.lay.nav_programs, glyph::APPS, tr("page.programs")),
            (Page::Settings, self.lay.nav_settings, glyph::SETTINGS, tr("page.settings")),
        ]
    }

    /// The compact NavigationView: an icon per page, the current one marked by an accent pill.
    pub(super) fn draw_rail(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        for (page, r, icon, _) in self.rail() {
            let hit = Hit::Nav(page);
            if self.page == page || self.hover == hit {
                g.fill(r, 4.0, th.control_hover());
            }
            if self.page == page {
                g.fill(R::new(r.l, r.t + 12.0, 3.0, 16.0), 1.5, th.accent());
            }
            g.icon(icon, Font::Icon16, r, th.text());
            // A dot on Settings while a newer release waits there.
            if page == Page::Settings && self.update.is_some() {
                g.circle(r.r - 14.0, r.t + 10.0, 4.0, th.accent());
            }
            self.hits.push((r, hit));
        }
    }

    /// The name of a rail item under the pointer, as the compact rail's tooltip shows it.
    pub(super) fn draw_rail_tip(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let Some((_, r, _, label)) = self.rail().into_iter().find(|(p, ..)| self.hover == Hit::Nav(*p)) else {
            return;
        };
        let w = g.measure(label, Font::Body) + 20.0;
        let tip = R::new(r.r + 6.0, r.t + 6.0, w, 28.0);
        g.fill(tip, 4.0, th.control_solid());
        g.stroke(tip, 4.0, th.strong_stroke(), 1.0);
        g.text(label, Font::Body, tip, Align::Center, th.text());
    }

    /// Which programs hear Goby, and whether each one does right now.
    pub(super) fn draw_programs(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let title = self.lay.title;
        g.text(tr("page.programs"), Font::Title, title, Align::Start, th.text());
        let scroll = self.settings_scroll.value.min(self.settings_max);
        let (x, w) = (self.lay.pane.l, self.lay.pane.w() - GUTTER);
        let mut y = self.lay.pane.t - scroll;
        g.clip(self.lay.pane);

        let note = tr("programs.note");
        g.text(note, Font::Body, R::new(x + 2.0, y, w, 22.0), Align::Start, th.text_secondary());
        y += 36.0;

        match &self.cable {
            // Setup in three steps; the last one ticks itself once the cable shows up.
            None => {
                let title = tr("programs.setup");
                g.text(title, Font::Strong, R::new(x + 2.0, y, w, 20.0), Align::Start, th.text());
                y += 24.0;
                let why = tr("programs.setup.desc");
                g.text(why, Font::Caption, R::new(x + 2.0, y, w, 18.0), Align::Start, th.text_secondary());
                y += 28.0;
                let (title, desc) = (tr("programs.step1"), tr("programs.step1.desc"));
                self.step_card(g, &mut y, 1, self.cable_opened, Hit::CableSite, title, desc);
                let (title, desc) = (tr("programs.step2"), tr("programs.step2.desc"));
                self.step_card(g, &mut y, 2, false, Hit::Downloads, title, desc);
                let (title, desc) = (tr("programs.step3"), tr("programs.step3.desc"));
                self.step_card(g, &mut y, 3, false, Hit::None, title, desc);
            }
            Some((_, record)) => {
                let fresh =
                    self.cable_found.is_some_and(|t| self.host.now().saturating_sub(t) < Duration::from_secs(30));
                let ready = match fresh {
                    true => tr("programs.cable.found"),
                    false => tr("programs.cable"),
                };
                let name = short(&record.name).to_string();
                let icon = if fresh { glyph::CHECK } else { glyph::MIC };
                self.row_card(g, &mut y, Hit::None, icon, &name, ready);
            }
        }
        y += 20.0;

        let section = |g: &dyn Canvas, y: &mut f32, title: &str| {
            g.text(title, Font::Strong, R::new(x + 2.0, *y, w, 20.0), Align::Start, th.text());
            *y += 28.0;
        };
        section(g, &mut y, tr("programs.chosen"));
        for i in 0..self.settings.routed.len() {
            let exe = self.settings.routed[i].clone();
            let talking = self.app_live.get(i).copied().unwrap_or(false);
            let elsewhere = self.app_elsewhere.get(i).cloned().flatten();
            let desc = match (self.app_state.get(i).copied().unwrap_or(AppState::Waiting), talking, elsewhere) {
                // Recording, but from a real mic: it chose one in its own settings, and only
                // switching that to Default lets Windows hand it Goby.
                (_, false, Some(mic)) => trf("programs.elsewhere", &[("mic", &mic)]),
                (_, true, _) => tr("programs.live").into(),
                (AppState::OnGoby, ..) => tr("programs.on").into(),
                (AppState::Waiting, ..) => tr("programs.waiting").into(),
                (AppState::Pending, ..) => tr("programs.pending").into(),
            };
            self.row_card(g, &mut y, Hit::Remove(i), Icon::Exe(&exe), &crate::host::display_name(&exe), &desc);
        }
        let hint = if self.settings.routed.is_empty() { tr("programs.add.first") } else { tr("programs.add.desc") };
        self.row_card(g, &mut y, Hit::AddApp, glyph::ADD, tr("programs.add"), hint);

        if !self.mic_apps.is_empty() {
            y += 20.0;
            section(g, &mut y, tr("programs.recent"));
            for i in 0..self.mic_apps.len().min(6) {
                let app = &self.mic_apps[i];
                let (name, exe) = (app.name.clone(), app.exe.clone());
                let desc = match app.active {
                    true => tr("programs.recent.active"),
                    false => tr("programs.recent.idle"),
                };
                self.row_card(g, &mut y, Hit::Offer(i), Icon::Exe(&exe), &name, desc);
            }
        }

        g.unclip();
        let content = y + scroll - self.lay.pane.t;
        self.settings_max = (content - self.lay.pane.h()).max(0.0);
        self.settings_scroll.target = self.settings_scroll.target.min(self.settings_max);
        self.scrollbar(g, self.lay.pane, scroll, self.settings_max, Pane::Settings);
    }

    /// What gets changed while playing: pitch, volume and hearing yourself, side by side.
    pub(super) fn draw_quick(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let r = self.lay.quick;
        self.card(g, r, 8.0, false);
        let w = r.w() / 3.0;
        for (i, (k, icon, title)) in
            [(Knob::Tone, glyph::TONE, tr("quick.pitch")), (Knob::Volume, glyph::VOLUME, tr("quick.volume"))]
                .into_iter()
                .enumerate()
        {
            let s = R::new(r.l + w * i as f32, r.t, w, r.h());
            g.icon(icon, Font::Icon16, R::new(s.l + 16.0, s.t + 12.0, 20.0, 24.0), th.text());
            g.text(title, Font::Body, R::new(s.l + 48.0, s.t + 12.0, s.w() - 140.0, 24.0), Align::Start, th.text());
            let value = R::new(s.r - 96.0, s.t + 12.0, 80.0, 24.0);
            g.text(&self.readout(k), Font::Caption, value, Align::End, th.text_secondary());
            self.slider(g, k, R::new(s.l + 26.0, s.t + 48.0, s.w() - 52.0, 24.0));
            g.fill(R::new(s.r - 0.5, s.t + 16.0, 1.0, s.h() - 32.0), 0.0, th.card_stroke());
        }
        let s = R::new(r.l + w * 2.0, r.t, w, r.h());
        g.icon(glyph::HEADPHONE, Font::Icon16, R::new(s.l + 16.0, s.t + 30.0, 20.0, 24.0), th.text());
        let text_w = s.w() - 48.0 - 72.0;
        let hear = tr("quick.hear");
        g.text(hear, Font::Body, R::new(s.l + 48.0, s.t + 20.0, text_w, 22.0), Align::Start, th.text());
        let mon = tr("quick.hear.desc");
        g.text(mon, Font::Caption, R::new(s.l + 48.0, s.t + 42.0, text_w, 18.0), Align::Start, th.text_secondary());
        self.toggle(g, s.r - 60.0, s.t + 32.0, Switch::Monitor);
        self.hits.push((s.inset(4.0), Hit::Toggle(Switch::Monitor)));
    }

    pub(super) fn draw_scope(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let r = self.lay.scope;
        self.card(g, r, 8.0, false);
        let def = &voices::ALL[self.voice];

        // A voice tuned away from the catalog can be kept; once kept it says where it went.
        // Both sit left of the Effect switch, and the name and the line under it end before
        // them, however narrow the window.
        let kept = self.active_preset().is_some();
        let (saved, save) = (tr("scope.saved"), tr("scope.save"));
        let aside = if kept {
            let w = g.measure(saved, Font::Caption) + 22.0;
            Some(R::new(r.r - 176.0 - w, r.t + 14.0, w, 24.0))
        } else if self.tuned() {
            // A WinUI button: 14 DIPs either side, the icon 8 DIPs before the text.
            let w = 14.0 + 12.0 + 8.0 + g.measure(save, Font::Body) + 14.0;
            Some(R::new(r.r - 176.0 - w, r.t + 10.0, w, 32.0))
        } else {
            None
        };
        let right = aside.map_or(r.r - 176.0, |a| a.l) - 12.0;
        let text_w = |max: f32| (right - (r.l + 20.0)).clamp(0.0, max);

        g.text(
            &self.label(self.current()),
            Font::Subtitle,
            R::new(r.l + 20.0, r.t + 12.0, text_w(380.0), 28.0),
            Align::Start,
            th.text(),
        );
        // With nobody listening the voice goes nowhere; say so where it is seen.
        let unheard = self.healthy && (self.cable.is_none() || self.settings.routed.is_empty());
        let (line, color) = if !self.healthy {
            (self.status.clone(), argb(if th.dark { 0xFFFF99A4 } else { 0xFFC42B1C }))
        } else if self.cable.is_none() {
            let none = tr("scope.no_cable");
            (none.into(), th.accent())
        } else if self.settings.routed.is_empty() {
            let none = tr("scope.nobody");
            (none.into(), th.accent())
        } else if !self.settings.enabled {
            let off = tr("scope.off");
            (off.into(), th.text_secondary())
        } else {
            let ms = self.latency.round().to_string();
            (trf("scope.latency", &[("category", def.category.name()), ("ms", &ms)]), th.text_secondary())
        };
        let line_r = R::new(r.l + 20.0, r.t + 40.0, text_w(420.0), 18.0);
        g.text(&line, Font::Caption, line_r, Align::Start, color);
        if unheard {
            self.hits.push((line_r, Hit::Nav(Page::Programs)));
        }

        match aside {
            Some(at) if kept => {
                g.icon(glyph::STAR_FILLED, Font::Icon12, R::new(at.l, at.t, 16.0, 24.0), th.accent());
                g.text(saved, Font::Caption, R { l: at.l + 22.0, ..at }, Align::Start, th.text_secondary());
            }
            Some(button) => {
                let hovered = self.hover == Hit::SavePreset;
                g.fill(button, 4.0, if hovered { th.control_hover() } else { th.control_alt() });
                g.stroke(button, 4.0, th.control_stroke(), 1.0);
                g.icon(glyph::STAR, Font::Icon12, R::new(button.l + 14.0, button.t, 12.0, 32.0), th.text());
                let label = R { l: button.l + 34.0, r: button.r - 14.0, ..button };
                g.text(save, Font::Body, label, Align::Start, th.text());
                self.hits.push((button, Hit::SavePreset));
            }
            None => {}
        }

        let label = tr("scope.effect");
        g.text(label, Font::Body, R::new(r.r - 160.0, r.t + 14.0, 88.0, 24.0), Align::End, th.text());
        self.toggle(g, r.r - 60.0, r.t + 16.0, Switch::Effect);
        self.hits.push((R::new(r.r - 170.0, r.t + 8.0, 156.0, 36.0), Hit::Toggle(Switch::Effect)));

        let area = R::new(r.l + 20.0, r.t + 64.0, r.w() - 40.0, r.h() - 76.0);
        g.line(&trace(&self.raw, area), th.text_tertiary(), 1.2);
        g.line(&trace(&self.voiced, area), th.accent(), 2.0);
    }

    pub(super) fn draw_tabs(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let mut items: Vec<(Shelf, &'static str)> =
            vec![(Shelf::All, tr("shelf.all")), (Shelf::Favorites, tr("shelf.favorites"))];
        items.extend(Category::ALL.iter().map(|c| (Shelf::Group(*c), c.name())));
        if self.tab_widths.len() != items.len() {
            self.tab_widths = items.iter().map(|(_, s)| g.measure(s, Font::Strong)).collect();
        }
        let total: f32 = self.tab_widths.iter().map(|w| w + 24.0).sum();
        self.tab_max = (total - self.lay.tabs.w()).max(0.0);
        self.tab_scroll = self.tab_scroll.min(self.tab_max);
        let mut x = self.lay.tabs.l - self.tab_scroll;
        g.clip(self.lay.tabs.inset(-4.0));
        for (i, (cat, label)) in items.iter().enumerate() {
            let w = self.tab_widths[i] + 24.0;
            let r = R::new(x, self.lay.tabs.t, w, self.lay.tabs.h());
            let selected = self.shelf == *cat;
            let color = if selected || self.hover == Hit::Tab(*cat) { th.text() } else { th.text_secondary() };
            g.text(label, if selected { Font::Strong } else { Font::Body }, r, Align::Center, color);
            if selected {
                let (cx, iw) = (x + w / 2.0, 16.0);
                if self.tab_w.target == 0.0 {
                    self.tab_x = Anim { value: cx, target: cx };
                    self.tab_w = Anim { value: iw, target: iw };
                }
                self.tab_x.target = cx;
                self.tab_w.target = iw;
            }
            if r.r > self.lay.tabs.l && r.l < self.lay.tabs.r {
                let visible = R { l: r.l.max(self.lay.tabs.l), r: r.r.min(self.lay.tabs.r), ..r };
                self.hits.push((visible, Hit::Tab(*cat)));
            }
            x += w;
        }
        // The SelectorBar indicator slides to the chosen category.
        let (cx, iw) = (self.tab_x.value, self.tab_w.value);
        g.fill(R::new(cx - iw / 2.0, self.lay.tabs.t + 33.0, iw, 3.0), 1.5, th.accent());
        g.unclip();
    }

    pub(super) fn draw_gallery(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let list = self.filtered();
        let tile_w = (self.lay.gallery.w() - GUTTER - TILE_GAP * (self.lay.cols - 1) as f32) / self.lay.cols as f32;
        let rows = list.len().div_ceil(self.lay.cols);
        let content = rows as f32 * (TILE_H + TILE_GAP) - TILE_GAP;
        self.gallery_max = (content - self.lay.gallery.h()).max(0.0);
        self.gallery_scroll.target = self.gallery_scroll.target.min(self.gallery_max);
        let scroll = self.gallery_scroll.value.min(self.gallery_max);

        g.clip(self.lay.gallery);
        for (n, &f) in list.iter().enumerate() {
            let (row, col) = (n / self.lay.cols, n % self.lay.cols);
            let r = R::new(
                self.lay.gallery.l + col as f32 * (tile_w + TILE_GAP),
                self.lay.gallery.t + row as f32 * (TILE_H + TILE_GAP) - scroll,
                tile_w,
                TILE_H,
            );
            if !r.intersects(&self.lay.gallery) {
                continue;
            }
            let (hit, corner) = match f {
                Fav::Voice(i) => (Hit::Tile(i), Hit::Star(i)),
                Fav::Preset(p) => (Hit::Preset(p), Hit::DropPreset(p)),
            };
            let selected = self.is_chosen(f);
            let hovered = self.hover == hit;
            g.fill(r, 4.0, if hovered { th.control_hover() } else { th.card() });
            if selected {
                g.stroke(r, 4.0, th.accent(), 2.0);
            } else {
                g.stroke(r, 4.0, th.card_stroke(), 1.0);
            }
            // The icon is the voice's own waveform.
            let wave = &self.glyphs[self.voice_of(f)];
            let (gl, gw, gy) = (r.l + 14.0, r.w() - 28.0, r.t + 24.0);
            let last = (wave.len() - 1) as f32;
            let pts: Vec<(f32, f32)> =
                wave.iter().enumerate().map(|(k, s)| (gl + gw * k as f32 / last, gy - s * 11.0)).collect();
            g.line(&pts, if selected { th.accent() } else { th.text_secondary() }, 1.4);
            // A saved voice puts what it changed on a line of its own, so the name and
            // the tweaks both fit however narrow the tile is.
            let tweaks = match f {
                Fav::Preset(p) => self.settings.presets.get(p).map(library::preset_tweaks),
                Fav::Voice(_) => None,
            };
            let width = r.w() - 28.0 - 24.0;
            if let Some(tweaks) = tweaks.filter(|t| !t.is_empty()) {
                let name = voices::ALL[self.voice_of(f)].name();
                g.text(name, Font::Body, R::new(r.l + 14.0, r.t + 38.0, width, 20.0), Align::Start, th.text());
                let second = R::new(r.l + 14.0, r.t + 55.0, width, 16.0);
                g.text(&tweaks, Font::Caption, second, Align::Start, th.text_secondary());
            } else {
                let name = R::new(r.l + 14.0, r.t + 44.0, width, 22.0);
                g.text(&self.label(f), Font::Body, name, Align::Start, th.text());
            }
            let visible = R { l: r.l, t: r.t.max(self.lay.gallery.t), r: r.r, b: r.b.min(self.lay.gallery.b) };
            self.hits.push((visible, hit));
            // A voice has a star: filled for a favourite, an outline offering to make it one.
            // A saved voice is always a favourite; on hover its corner deletes it.
            let button = R::new(r.r - 34.0, r.b - 34.0, 28.0, 28.0);
            let on_button = self.hover == corner;
            let shown = match f {
                Fav::Voice(i) if self.is_favorite(i) => Some((glyph::STAR_FILLED, th.accent())),
                Fav::Voice(_) if hovered || on_button => Some((glyph::STAR, th.text_secondary())),
                Fav::Preset(_) if hovered || on_button => Some((glyph::REMOVE, th.text_secondary())),
                Fav::Preset(_) => Some((glyph::STAR_FILLED, th.accent())),
                Fav::Voice(_) => None,
            };
            if let Some((icon, color)) = shown {
                if on_button {
                    g.fill(button, 4.0, th.control_hover());
                }
                g.icon(icon, Font::Icon12, button, color);
                if (hovered || on_button) && visible.has((button.l + 14.0, button.t + 14.0)) {
                    self.hits.push((button, corner));
                }
            }
        }
        if list.is_empty() && self.shelf == Shelf::Favorites {
            let (l, t, w) = (self.lay.gallery.l + 2.0, self.lay.gallery.t + 8.0, self.lay.gallery.w() - GUTTER);
            let first = tr("favorites.empty");
            g.text(first, Font::Strong, R::new(l, t, w, 22.0), Align::Start, th.text());
            let second = tr("favorites.empty.desc");
            g.text(second, Font::Body, R::new(l, t + 26.0, w, 22.0), Align::Start, th.text_secondary());
        }
        g.unclip();
        self.scrollbar(g, self.lay.gallery, scroll, self.gallery_max, Pane::Gallery);
    }

    pub(super) fn draw_settings(&mut self, g: &dyn Canvas) {
        let th = self.theme;
        let title = self.lay.title;
        g.text(tr("page.settings"), Font::Title, title, Align::Start, th.text());

        let scroll = self.settings_scroll.value.min(self.settings_max);
        let (x, w) = (self.lay.pane.l, self.lay.pane.w() - GUTTER);
        let mut y = self.lay.pane.t - scroll;
        g.clip(self.lay.pane);

        let section = |g: &dyn Canvas, y: &mut f32, title: &str| {
            g.text(title, Font::Strong, R::new(x + 2.0, *y, w, 20.0), Align::Start, th.text());
            *y += 28.0;
        };
        self.update_card(g, &mut y);

        section(g, &mut y, tr("settings.mic"));
        let mic = self.inputs.get(self.input).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Mic), glyph::MIC, tr("settings.mic.device"), &mic);
        let gate = tr("settings.gate.desc");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Gate), glyph::GATE, tr("settings.gate"), gate);
        // Rarely touched once set: folded away like an expander in Windows Settings.
        let more = tr("settings.advanced");
        let more_desc = tr("settings.advanced.desc");
        self.row_card(g, &mut y, Hit::Advanced, glyph::MORE, more, more_desc);
        if self.advanced {
            let fast = tr("settings.exclusive");
            let mode = match (self.settings.exclusive, self.host.exclusive()) {
                (true, true) => tr("settings.exclusive.on"),
                (true, false) => tr("settings.exclusive.refused"),
                _ => tr("settings.exclusive.off"),
            };
            self.row_card(g, &mut y, Hit::Toggle(Switch::Exclusive), glyph::SPEED, fast, mode);
            self.slider_card(g, &mut y, Knob::Gain, glyph::VOLUME, tr("settings.gain"));
            self.slider_card(g, &mut y, Knob::Gate, glyph::GATE, tr("settings.gate_db"));
        }
        y += 20.0;

        section(g, &mut y, tr("settings.output"));
        let out = self.outputs.get(self.output).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Out), glyph::SPEAKERS, tr("settings.output.device"), &out);
        self.slider_card(g, &mut y, Knob::Mix, glyph::MIX, tr("settings.mix"));
        y += 20.0;

        section(g, &mut y, tr("settings.general"));
        let language = self.language_name(&self.settings.lang);
        self.row_card(g, &mut y, Hit::Pick(Pick::Lang), glyph::GLOBE, tr("settings.language"), &language);
        let tray = tr("settings.tray.title");
        let tray_desc = tr("settings.tray.desc");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Tray), glyph::MINIMIZE, tray, tray_desc);
        let start = tr("settings.autostart.title");
        let start_desc = tr("settings.autostart.desc");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Autostart), glyph::POWER, start, start_desc);
        let keys = tr("settings.hotkeys.title");
        let keys_desc = match self.hotkeys_ok {
            true => tr("settings.hotkeys.desc"),
            false => tr("settings.hotkeys.taken"),
        };
        self.row_card(g, &mut y, Hit::Toggle(Switch::Hotkeys), glyph::KEYBOARD, keys, keys_desc);
        let check = tr("settings.updates.title");
        let check_desc = tr("settings.updates.desc");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Updates), glyph::UPDATE, check, check_desc);

        y += 20.0;
        // The commit says which code this copy claims to be built from; the signature on
        // the release, not this line, is what says the copy is genuine.
        let about = match option_env!("GOBY_COMMIT").unwrap_or("") {
            "" => format!("Goby {VERSION}"),
            commit => format!("Goby {VERSION} · {commit}"),
        };
        self.row_card(g, &mut y, Hit::About, glyph::INFO, &about, "Azer Sadykhzadeh · azer.one");

        g.unclip();
        let content = y + scroll - self.lay.pane.t;
        self.settings_max = (content - self.lay.pane.h()).max(0.0);
        self.settings_scroll.target = self.settings_scroll.target.min(self.settings_max);
        self.scrollbar(g, self.lay.pane, scroll, self.settings_max, Pane::Settings);
    }

    /// The card that offers a newer release, at the top of Settings.
    fn update_card(&mut self, g: &dyn Canvas, y: &mut f32) {
        let Some(version) = &self.update else { return };
        let title = trf("update.out", &[("version", version)]);
        let desc = match (self.updating, self.host.installed()) {
            (Updating::Downloading, _) => tr("update.downloading"),
            (Updating::Failed, _) => tr("update.failed"),
            (_, true) => tr("update.install"),
            (_, false) => tr("update.download"),
        };
        let hit = if self.updating == Updating::Downloading { Hit::None } else { Hit::Update };
        self.row_card(g, y, hit, glyph::UPDATE, &title, desc);
        *y += 20.0;
    }
}
