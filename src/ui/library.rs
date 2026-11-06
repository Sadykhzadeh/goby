//! Favourites and saved voices: what the gallery shows on each shelf and what a pick does.

use super::*;

impl App {
    /// Saved voices first, then the starred ones, each in the order it was added.
    pub(super) fn favorites(&self) -> Vec<Fav> {
        let saved = (0..self.settings.presets.len())
            .filter(|p| voices::index_of(&self.settings.presets[*p].voice).is_some())
            .map(Fav::Preset);
        let starred = self.settings.favorites.iter().filter_map(|id| voices::index_of(id)).map(Fav::Voice);
        saved.chain(starred).collect()
    }

    /// The saved voice that is set exactly as it is playing now, if any.
    pub(super) fn active_preset(&self) -> Option<usize> {
        let s = &self.settings;
        let id = voices::ALL[self.voice].id;
        s.presets.iter().position(|p| p.matches(id, s.semitones, s.dry_mix, s.volume))
    }

    pub(super) fn current(&self) -> Fav {
        self.active_preset().map_or(Fav::Voice(self.voice), Fav::Preset)
    }

    /// The voice a tile plays, for its waveform.
    pub(super) fn voice_of(&self, f: Fav) -> usize {
        match f {
            Fav::Voice(i) => i,
            Fav::Preset(p) => self.settings.presets.get(p).and_then(|p| voices::index_of(&p.voice)).unwrap_or(0),
        }
    }

    pub(super) fn label(&self, f: Fav) -> String {
        match f {
            Fav::Voice(i) => voices::ALL[i].name(self.ru).to_string(),
            Fav::Preset(p) => {
                let name = voices::ALL[self.voice_of(f)].name(self.ru);
                self.settings.presets.get(p).map_or(name.to_string(), |p| preset_label(name, p, self.ru))
            }
        }
    }

    /// Switches to a tile. `wake` also turns the effect on, for picks made outside the
    /// gallery, from a hotkey or the tray, where wanting a voice means wanting to hear it.
    pub(super) fn choose(&mut self, f: Fav, wake: bool) {
        match f {
            Fav::Voice(i) => {
                self.voice = i;
                self.settings.semitones = 0.0;
            }
            Fav::Preset(p) => {
                let Some(preset) = self.settings.presets.get(p).cloned() else { return };
                let Some(i) = voices::index_of(&preset.voice) else { return };
                self.voice = i;
                self.settings.semitones = preset.semitones;
                self.settings.dry_mix = preset.dry_mix;
                self.settings.volume = preset.volume;
            }
        }
        if wake {
            self.settings.enabled = true;
        }
        self.apply();
        self.settings.save();
        self.dirty = true;
    }

    /// The current voice has been tuned away from how the catalog plays it.
    pub(super) fn tuned(&self) -> bool {
        let s = &self.settings;
        s.semitones != 0.0 || s.dry_mix != 0.0 || (s.volume - 1.0).abs() > 0.005
    }

    pub(super) fn save_preset(&mut self) {
        if self.active_preset().is_some() {
            return;
        }
        let s = &self.settings;
        let preset = Preset {
            voice: voices::ALL[self.voice].id.to_string(),
            semitones: s.semitones,
            dry_mix: s.dry_mix,
            volume: s.volume,
        };
        self.settings.presets.push(preset);
        self.settings.save();
        self.dirty = true;
    }

    pub(super) fn drop_preset(&mut self, p: usize) {
        if p < self.settings.presets.len() {
            self.settings.presets.remove(p);
            self.settings.save();
            self.hover = Hit::None;
            self.dirty = true;
        }
    }

    pub(super) fn is_favorite(&self, i: usize) -> bool {
        self.settings.favorites.iter().any(|id| id == voices::ALL[i].id)
    }

    pub(super) fn toggle_favorite(&mut self, i: usize) {
        let id = voices::ALL[i].id;
        match self.is_favorite(i) {
            true => self.settings.favorites.retain(|f| f != id),
            false => self.settings.favorites.push(id.to_string()),
        }
        self.settings.save();
        self.dirty = true;
    }

    pub(super) fn filtered(&self) -> Vec<Fav> {
        match self.shelf {
            Shelf::All => (0..voices::ALL.len()).map(Fav::Voice).collect(),
            Shelf::Favorites => self.favorites(),
            Shelf::Group(c) => {
                (0..voices::ALL.len()).filter(|i| voices::ALL[*i].category == c).map(Fav::Voice).collect()
            }
        }
    }

    /// The tile drawn as chosen: a saved voice while it plays exactly as saved, otherwise
    /// the catalog voice, except on Favorites, where that would mark two tiles.
    pub(super) fn is_chosen(&self, f: Fav) -> bool {
        match f {
            Fav::Preset(p) => self.active_preset() == Some(p),
            Fav::Voice(i) => i == self.voice && (self.shelf != Shelf::Favorites || self.active_preset().is_none()),
        }
    }
}

/// A saved voice's name: the voice and what was changed, e.g. "Demon +3 · 20% own".
pub(super) fn preset_label(name: &str, p: &Preset, ru: bool) -> String {
    let mut label = name.to_string();
    if p.semitones != 0.0 {
        label.push_str(&format!(" {:+}", p.semitones));
    }
    if (p.volume - 1.0).abs() > 0.005 {
        label.push_str(&format!(" · {}%", (p.volume * 100.0).round()));
    }
    if p.dry_mix > 0.0 {
        let own = if ru { "своего" } else { "own" };
        label.push_str(&format!(" · {}% {own}", (p.dry_mix * 100.0).round()));
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_voice_is_named_after_what_was_changed() {
        let p = |semitones, dry_mix, volume| Preset { voice: "demon".into(), semitones, dry_mix, volume };
        assert_eq!(preset_label("Demon", &p(3.0, 0.0, 1.0), false), "Demon +3");
        assert_eq!(preset_label("Demon", &p(-2.5, 0.2, 1.0), false), "Demon -2.5 · 20% own");
        assert_eq!(preset_label("Демон", &p(0.0, 0.0, 1.25), true), "Демон · 125%");
    }
}
