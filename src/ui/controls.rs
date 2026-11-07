//! WinUI-style controls: cards, toggles, sliders and scrollbars.

use super::*;

impl<H: Host> App<H> {
    pub(super) fn card(&self, g: &dyn Canvas, r: R, radius: f32, hovered: bool) {
        let th = &self.theme;
        g.fill(r, radius, if hovered { th.control_hover() } else { th.card() });
        g.stroke(r, radius, th.card_stroke(), 1.0);
    }

    pub(super) fn toggle(&self, g: &dyn Canvas, x: f32, y: f32, which: Switch) {
        let t = self.switches[which as usize].value;
        self.toggle_at(g, x, y, t, self.hover == Hit::Toggle(which));
    }

    /// WinUI ToggleSwitch: 40x20 track, 12 px knob that slides and recolours.
    pub(super) fn toggle_at(&self, g: &dyn Canvas, x: f32, y: f32, t: f32, hovered: bool) {
        let th = &self.theme;
        let track = R::new(x, y, 40.0, 20.0);
        if t < 1.0 {
            g.fill(track, 10.0, with_alpha(th.control_alt(), 1.0 - t));
            g.stroke(track, 10.0, with_alpha(th.strong_stroke(), 1.0 - t), 1.0);
        }
        if t > 0.0 {
            g.fill(track, 10.0, with_alpha(th.accent(), t));
        }
        let radius = if hovered { 7.0 } else { 6.0 };
        let knob = if t > 0.5 { th.on_accent() } else { th.text_secondary() };
        g.circle(x + 10.0 + 20.0 * t, y + 10.0, radius, knob);
    }

    /// WinUI Slider: 4 px rail, accent fill from the origin, raised thumb with an accent dot.
    pub(super) fn slider(&mut self, g: &dyn Canvas, k: Knob, track: R) {
        let th = self.theme;
        let (lo, hi, _, _) = k.range();
        let origin = if k == Knob::Tone { 0.0 } else { lo };
        let at = |v: f32| track.l + (v - lo) / (hi - lo) * track.w();
        let (x0, x1) = (at(origin), at(self.knob(k)));
        let yc = track.t + track.h() / 2.0;
        let dim = match k {
            Knob::Gate => !self.settings.gate,
            Knob::Gain => false,
            _ => !self.settings.enabled,
        };
        let accent = if dim { th.text_tertiary() } else { th.accent() };
        g.fill(R::new(track.l, yc - 2.0, track.w(), 4.0), 2.0, th.strong_stroke());
        g.fill(R { l: x0.min(x1), t: yc - 2.0, r: x0.max(x1), b: yc + 2.0 }, 2.0, accent);
        g.circle(x1, yc, 10.0, th.control_stroke());
        g.circle(x1, yc, 9.0, th.control_solid());
        let pressed = self.drag.as_ref().is_some_and(|d| d.hit == Hit::Slider(k));
        let hovered = self.hover == Hit::Slider(k);
        g.circle(
            x1,
            yc,
            if pressed {
                5.0
            } else if hovered {
                7.0
            } else {
                6.0
            },
            accent,
        );
        self.tracks.push((k, track));
        self.hits.push((R { l: track.l - 10.0, t: yc - 12.0, r: track.r + 10.0, b: yc + 12.0 }, Hit::Slider(k)));
    }

    /// Thin overlay scrollbar, wider while hovered or dragged, like WinUI's.
    /// Out of line: each page draws one.
    #[inline(never)]
    pub(super) fn scrollbar(&mut self, g: &dyn Canvas, view: R, scroll: f32, max: f32, pane: Pane) {
        if max <= 0.0 {
            return;
        }
        let th = &self.theme;
        let content = view.h() + max;
        let thumb = (view.h() * view.h() / content).max(32.0);
        let y = view.t + scroll / max * (view.h() - thumb);
        let active =
            self.hover == Hit::Scrollbar(pane) || self.drag.as_ref().is_some_and(|d| d.hit == Hit::Scrollbar(pane));
        let w = if active { 6.0 } else { 3.0 };
        g.fill(R::new(view.r - 4.0 - w / 2.0, y, w, thumb), w / 2.0, th.strong_stroke());
        self.hits.push((R::new(view.r - 12.0, view.t, 12.0, view.h()), Hit::Scrollbar(pane)));
    }

    /// SettingsCard with an icon, a title and a value, and a slider underneath.
    pub(super) fn slider_card(&mut self, g: &dyn Canvas, y: &mut f32, k: Knob, icon: u16, title: &str) {
        let r = R::new(self.lay.pane.l, *y, self.lay.pane.w() - GUTTER, 84.0);
        if r.intersects(&self.lay.pane) {
            self.card(g, r, 4.0, false);
            self.knob_block(g, r, k, icon, title, 52.0);
        }
        *y += 84.0 + CARD_GAP;
    }

    /// A knob's icon, title and value over its slider, filling `r`; the title starts
    /// `title_x` in. Shared by the quick card and the slider cards in Settings.
    #[inline(never)]
    pub(super) fn knob_block(&mut self, g: &dyn Canvas, r: R, k: Knob, icon: u16, title: &str, title_x: f32) {
        let th = self.theme;
        g.icon(icon, Font::Icon16, R::new(r.l + 16.0, r.t + 12.0, 20.0, 24.0), th.text());
        let title_r = R::new(r.l + title_x, r.t + 12.0, r.w() - 140.0, 24.0);
        g.text(title, Font::Body, title_r, Align::Start, th.text());
        let value = R::new(r.r - 96.0, r.t + 12.0, 80.0, 24.0);
        g.text(&self.readout(k), Font::Caption, value, Align::End, th.text_secondary());
        self.slider(g, k, R::new(r.l + 26.0, r.t + 48.0, r.w() - 52.0, 24.0));
    }

    /// One step of a setup: its number in a circle, a check mark once done, and the
    /// action it opens on the right when there is one.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn step_card(
        &mut self,
        g: &dyn Canvas,
        y: &mut f32,
        n: usize,
        done: bool,
        hit: Hit,
        title: &str,
        desc: &str,
    ) {
        let th = self.theme;
        let r = R::new(self.lay.pane.l, *y, self.lay.pane.w() - GUTTER, 68.0);
        if r.intersects(&self.lay.pane) {
            self.card(g, r, 4.0, hit != Hit::None && self.hover == hit);
            let (cx, cy) = (r.l + 26.0, r.t + 34.0);
            let mark = R::new(cx - 12.0, cy - 12.0, 24.0, 24.0);
            if done {
                g.circle(cx, cy, 12.0, th.accent());
                g.icon(glyph::CHECK, Font::Icon12, mark, th.on_accent());
            } else {
                g.circle(cx, cy, 12.0, th.strong_stroke());
                g.circle(cx, cy, 11.0, th.card());
                g.text(&n.to_string(), Font::Strong, mark, Align::Center, th.text());
            }
            let text_w = r.w() - 52.0 - 64.0;
            g.text(title, Font::Body, R::new(r.l + 52.0, r.t + 13.0, text_w, 22.0), Align::Start, th.text());
            let desc_r = R::new(r.l + 52.0, r.t + 35.0, text_w, 18.0);
            g.text(desc, Font::Caption, desc_r, Align::Start, th.text_secondary());
            if hit != Hit::None {
                g.icon(glyph::OPEN, Font::Icon12, R::new(r.r - 36.0, r.t + 24.0, 20.0, 20.0), th.text_secondary());
                let visible = R { t: r.t.max(self.lay.pane.t), b: r.b.min(self.lay.pane.b), ..r };
                self.hits.push((visible, hit));
            }
        }
        *y += 68.0 + CARD_GAP;
    }

    /// SettingsCard row: icon, title, description, and a toggle or chevron on the right.
    pub(super) fn row_card<'a>(
        &mut self,
        g: &dyn Canvas,
        y: &mut f32,
        hit: Hit,
        icon: impl Into<Icon<'a>>,
        title: &str,
        desc: &str,
    ) {
        // One body for glyphs and program icons alike, rather than a copy for each.
        self.icon_row_card(g, y, hit, icon.into(), title, desc)
    }

    fn icon_row_card(&mut self, g: &dyn Canvas, y: &mut f32, hit: Hit, icon: Icon, title: &str, desc: &str) {
        let th = self.theme;
        let r = R::new(self.lay.pane.l, *y, self.lay.pane.w() - GUTTER, 68.0);
        if r.intersects(&self.lay.pane) {
            self.card(g, r, 4.0, self.hover == hit && !matches!(hit, Hit::Remove(_)));
            let spot = R::new(r.l + 16.0, r.t + 22.0, 20.0, 24.0);
            match icon {
                Icon::Glyph(code) => g.icon(code, Font::Icon16, spot, th.text()),
                Icon::Exe(exe) => {
                    // The program's own icon when its file is known and has one; the
                    // kind of program (game, chat, browser) as a glyph otherwise.
                    let px = (20.0 * g.dpi() / 96.0).round() as u32;
                    let path = self.exe_paths.iter().find(|(e, _)| e == exe).map(|(_, p)| p);
                    let pic = R::new(r.l + 16.0, r.t + 24.0, 20.0, 20.0);
                    let host = &self.host;
                    let drawn = path.is_some_and(|p| g.picture(exe, px, &mut || host.icon(p, px), pic));
                    if !drawn {
                        g.icon(app_glyph(exe), Font::Icon16, spot, th.text());
                    }
                }
            }
            let text_w = r.w() - 52.0 - 64.0;
            g.text(title, Font::Body, R::new(r.l + 52.0, r.t + 13.0, text_w, 22.0), Align::Start, th.text());
            g.text(
                desc,
                Font::Caption,
                R::new(r.l + 52.0, r.t + 35.0, text_w, 18.0),
                Align::Start,
                th.text_secondary(),
            );
            let side = R::new(r.r - 36.0, r.t + 24.0, 20.0, 20.0);
            match hit {
                Hit::Toggle(s) => self.toggle(g, r.r - 56.0, r.t + 24.0, s),
                Hit::Remove(_) => {
                    // A small close button; the card itself does nothing.
                    let b = R::new(r.r - 48.0, r.t + 18.0, 32.0, 32.0);
                    if self.hover == hit {
                        g.fill(b, 4.0, th.control_hover());
                    }
                    g.icon(glyph::REMOVE, Font::Icon12, b, th.text_secondary());
                }
                Hit::AddApp => g.icon(glyph::CHEVRON_DOWN, Font::Icon12, side, th.text_secondary()),
                Hit::Offer(_) => g.icon(glyph::ADD, Font::Icon12, side, th.text_secondary()),
                Hit::Advanced => {
                    let chevron = if self.advanced { glyph::CHEVRON_UP } else { glyph::CHEVRON_DOWN };
                    g.icon(chevron, Font::Icon12, side, th.text_secondary())
                }
                Hit::Pick(_) => g.icon(glyph::CHEVRON_DOWN, Font::Icon12, side, th.text_secondary()),
                Hit::None => {}
                _ => g.icon(glyph::OPEN, Font::Icon12, side, th.text_secondary()),
            }
            let target = if matches!(hit, Hit::Remove(_)) { R::new(r.r - 48.0, r.t + 18.0, 32.0, 32.0) } else { r };
            let visible = R { t: target.t.max(self.lay.pane.t), b: target.b.min(self.lay.pane.b), ..target };
            self.hits.push((visible, hit));
        }
        *y += 68.0 + CARD_GAP;
    }
}

/// What a settings row shows on its left: a Segoe Fluent glyph, or a program's own icon.
pub(super) enum Icon<'a> {
    Glyph(u16),
    Exe(&'a str),
}

impl From<u16> for Icon<'_> {
    fn from(code: u16) -> Self {
        Icon::Glyph(code)
    }
}
