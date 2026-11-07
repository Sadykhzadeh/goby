//! The window, laid out like Windows 11 Settings: cards, the accent colour, Segoe UI
//! Variable and a navigation rail. Everything here is the same in the app and on the
//! website: what the window shows, where, and what a click does. The system around it comes
//! through `Host`, and it is drawn on a `Canvas`. The app's Win32 window is in `win`.

use std::time::Duration;

use crate::draw::{Align, Canvas, Font, R, glyph};
use crate::host::{AppState, Endpoint, Host, Hotkey, Key, Link, MicApp, SCOPE_LEN};
use crate::lang::{self, tr, trf};
use crate::settings::{Preset, Settings, signed, write_decimal};
use crate::theme::{Theme, argb, with_alpha};
use crate::voices::{self, Category};
use goby_dsp::fx::clamp;

mod controls;
mod input;
mod library;
mod pages;
#[cfg(windows)]
mod win;
#[cfg(windows)]
pub use win::run;

// Layout, in DIPs.
/// Default and smallest client sizes, in DIPs.
pub const WIDTH: f32 = 1000.0;
pub const HEIGHT: f32 = 720.0;
pub const MIN_W: f32 = 820.0;
pub const MIN_H: f32 = 560.0;
/// Gallery tiles never get narrower than this; extra width buys more columns.
const TILE_MIN_W: f32 = 140.0;
const TILE_GAP: f32 = 8.0;
/// Room on the right of a scrolling pane for its scrollbar.
const GUTTER: f32 = 12.0;
const TILE_H: f32 = 76.0;
const CARD_GAP: f32 = 4.0;
/// The settings page reads as one column, like Windows Settings, however wide the window.
const SETTINGS_W: f32 = 760.0;
/// Width of the navigation rail, as in a compact WinUI NavigationView.
const RAIL_W: f32 = 56.0;

/// The version shown in Settings. The website's build names the app's, not its own.
const VERSION: &str = match option_env!("GOBY_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Knob {
    Tone,
    Volume,
    Mix,
    Gain,
    Gate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Switch {
    Effect,
    Gate,
    Monitor,
    Exclusive,
    Tray,
    Autostart,
    Hotkeys,
    Updates,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Mic,
    Out,
    Lang,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pane {
    Gallery,
    Settings,
}

/// One tile of the gallery: a catalog voice, or a voice someone saved with their own tuning.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fav {
    Voice(usize),
    Preset(usize),
}

/// Which voices the gallery shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shelf {
    All,
    Favorites,
    Group(Category),
}

/// The voices are the app; everything set once lives on its own page.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Voices,
    Programs,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    None,
    Tile(usize),
    Tab(Shelf),
    /// The star on a voice tile.
    Star(usize),
    /// A saved voice (index into `settings.presets`), and the × that deletes it.
    Preset(usize),
    DropPreset(usize),
    /// Save the current voice with its pitch, volume and mix.
    SavePreset,
    /// Unfold or fold the advanced microphone settings.
    Advanced,
    /// Install the newer release, or open its page for a portable copy.
    Update,
    Slider(Knob),
    Toggle(Switch),
    Pick(Pick),
    /// The author's site.
    About,
    /// Take a chosen program (index into `settings.routed`) off the list.
    Remove(usize),
    /// Choose another program that should hear Goby.
    AddApp,
    Scrollbar(Pane),
    /// Go to a page: the navigation rail, or the "nobody hears Goby" hint.
    Nav(Page),
    /// A recent microphone user (index into `mic_apps`) to add with one click.
    Offer(usize),
    /// The VB-Cable download page.
    CableSite,
    /// The Downloads folder, where the cable's archive lands.
    Downloads,
}

impl Knob {
    /// (minimum, maximum, step, reset value)
    fn range(self) -> (f32, f32, f32, f32) {
        match self {
            Knob::Tone => (-12.0, 12.0, 1.0, 0.0),
            Knob::Volume => (0.0, 2.0, 0.05, 1.0),
            Knob::Mix => (0.0, 1.0, 0.05, 0.0),
            Knob::Gain => (-12.0, 24.0, 1.0, 0.0),
            Knob::Gate => (-80.0, -10.0, 1.0, -50.0),
        }
    }
}

/// Where everything goes for the current window size.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    /// The navigation rail on the left: Voices on top, Settings pinned to the bottom.
    nav_voices: R,
    nav_programs: R,
    nav_settings: R,
    // The voices page.
    scope: R,
    quick: R,
    tabs: R,
    gallery: R,
    cols: usize,
    // The settings page: a title over one centred column.
    title: R,
    pane: R,
}

impl Layout {
    fn new(w: f32, h: f32) -> Layout {
        let (w, h) = (w.max(MIN_W), h.max(MIN_H));
        let nav_voices = R::new(4.0, 12.0, RAIL_W - 8.0, 40.0);
        let nav_programs = R::new(4.0, nav_voices.b + 4.0, RAIL_W - 8.0, 40.0);
        let nav_settings = R::new(4.0, h - 12.0 - 40.0, RAIL_W - 8.0, 40.0);
        let left = RAIL_W + 8.0;
        let full = w - left - 24.0;
        let scope = R::new(left, 16.0, full, 120.0);
        let quick = R::new(left, scope.b + CARD_GAP * 2.0, full, 84.0);
        let tabs = R::new(left, quick.b + 12.0, full, 36.0);
        let gallery = R { l: left, t: tabs.b + 8.0, r: left + full, b: h - 16.0 };
        let cols = (((gallery.w() - GUTTER + TILE_GAP) / (TILE_MIN_W + TILE_GAP)) as usize).clamp(2, 10);
        let column = full.min(SETTINGS_W);
        let title = R::new(left + (full - column) / 2.0, 16.0, column, 44.0);
        let pane = R { l: title.l, t: title.b + 8.0, r: title.r, b: h - 16.0 };
        Layout { nav_voices, nav_programs, nav_settings, scope, quick, tabs, gallery, cols, title, pane }
    }
}

/// A menu a click opens: the app shows a native one, the website its own. Choices are
/// reported by key (device id or exe name), not position, as the lists may change meanwhile.
pub struct Popup {
    pub pick: Option<Pick>,
    pub labels: Vec<String>,
    pub keys: Vec<String>,
    pub checked: Option<usize>,
    /// Label of the last item and title of the file dialog it opens, where there is one.
    pub browse: Option<(String, String)>,
    /// Where it opens, in the window's DIPs.
    pub at: (f32, f32),
}

struct Drag {
    hit: Hit,
    /// For scrollbars: pointer y and scroll offset when the drag started.
    from: (f32, f32),
}

/// Eases towards a target each frame; reports whether it is still moving.
#[derive(Clone, Copy, Default)]
struct Anim {
    value: f32,
    target: f32,
}

impl Anim {
    #[inline(never)]
    fn step(&mut self, rate: f32) -> bool {
        let d = self.target - self.value;
        if d.abs() < 0.001 {
            self.value = self.target;
            return false;
        }
        self.value += d * rate;
        true
    }
}

/// Where installing a newer release has got to.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Updating {
    Idle,
    Downloading,
    Failed,
}

pub struct App<H: Host> {
    pub host: H,
    lay: Layout,
    tab_scroll: f32,
    tab_max: f32,
    theme: Theme,
    dpi: f32,

    settings: Settings,
    inputs: Vec<Endpoint>,
    outputs: Vec<Endpoint>,
    input: usize,
    output: usize,
    voice: usize,
    shelf: Shelf,
    /// The advanced microphone settings are unfolded.
    advanced: bool,
    page: Page,

    status: String,
    healthy: bool,
    retry_at: Option<Duration>,
    /// The virtual cable (play side, record side), if installed.
    cable: Option<(Endpoint, Endpoint)>,
    /// Recent microphone users not chosen yet (offered by "Add"), and the state of each
    /// chosen program, in `settings.routed` order.
    mic_apps: Vec<MicApp>,
    app_state: Vec<AppState>,
    /// Each chosen program records from Goby right now, in `settings.routed` order.
    app_live: Vec<bool>,
    /// The real microphone a chosen program records from instead, when it picks its own.
    app_elsewhere: Vec<Option<String>>,
    /// Where each program seen so far lives (lower-case exe name to full path), for icons.
    exe_paths: Vec<(String, String)>,
    apps_at: Duration,
    /// The VB-Cable page was opened from the setup steps this session.
    cable_opened: bool,
    /// When a cable installed while Goby was open was found, to say so for a while.
    cable_found: Option<Duration>,
    /// Re-read device names a moment after the elevated rename was started.
    recheck_at: Option<Duration>,

    mouse: (f32, f32),
    hover: Hit,
    drag: Option<Drag>,
    hits: Vec<(R, Hit)>,
    tracks: Vec<(Knob, R)>,
    gallery_scroll: Anim,
    settings_scroll: Anim,
    /// How far the current page has come in: 0 just switched, 1 settled.
    page_in: Anim,
    gallery_max: f32,
    settings_max: f32,
    switches: [Anim; 8],
    /// Windows starts Goby at sign-in; kept in the registry, read once here.
    autostart: bool,
    /// Every hotkey registered; false when another program holds one.
    hotkeys_ok: bool,
    /// The version of a newer verified release, and where installing it has got to.
    update: Option<String>,
    updating: Updating,
    tab_x: Anim,
    tab_w: Anim,
    tab_widths: Vec<f32>,
    latency: f32,
    glyphs: Vec<Vec<f32>>,
    raw: Box<[f32; SCOPE_LEN]>,
    voiced: Box<[f32; SCOPE_LEN]>,
    /// Something changed since the last frame was drawn.
    pub dirty: bool,
    quiet_frames: u32,
    /// The window can be seen: shown, not minimised, not under a fullscreen game.
    pub shown: bool,
    /// The Programs page is on screen, so what each program records from is worth asking.
    watching: bool,
    /// Every animation jumps straight to its end (the website, for reduced motion).
    pub still: bool,
}

/// A Segoe Fluent icon for well-known programs; a game pad for everything else.
fn app_glyph(exe: &str) -> u16 {
    match exe {
        "discord.exe" | "telegram.exe" | "teams.exe" | "ms-teams.exe" | "zoom.exe" | "skype.exe" => glyph::CHAT,
        "chrome.exe" | "msedge.exe" | "firefox.exe" | "opera.exe" | "brave.exe" | "steamwebhelper.exe" => glyph::GLOBE,
        "obs64.exe" | "obs32.exe" => glyph::VIDEO,
        "nvidia broadcast.exe" => glyph::MIC,
        _ => glyph::GAME,
    }
}

/// The place in `list` of the first of `ids` that is in it, or 0.
fn first_present(list: &[Endpoint], ids: &[&str]) -> usize {
    ids.iter().filter(|id| !id.is_empty()).find_map(|id| list.iter().position(|e| e.id == *id)).unwrap_or(0)
}

// Out of line: the split's searcher is sizeable and every device and cable name goes through here.
#[inline(never)]
fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

impl<H: Host> App<H> {
    pub fn new(host: H, settings: Settings, theme: Theme, dpi: f32) -> App<H> {
        let mut app = App {
            lay: Layout::new(WIDTH, HEIGHT),
            tab_scroll: 0.0,
            tab_max: 0.0,
            theme,
            dpi,
            voice: voices::index_of(&settings.voice).unwrap_or(1),
            settings,
            inputs: Vec::new(),
            outputs: Vec::new(),
            input: 0,
            output: 0,
            shelf: Shelf::All,
            advanced: false,
            page: Page::Voices,
            status: String::new(),
            healthy: true,
            retry_at: None,
            cable: None,
            mic_apps: Vec::new(),
            app_state: Vec::new(),
            app_live: Vec::new(),
            app_elsewhere: Vec::new(),
            exe_paths: Vec::new(),
            apps_at: host.now(),
            recheck_at: None,
            cable_opened: false,
            cable_found: None,
            mouse: (-1.0, -1.0),
            hover: Hit::None,
            drag: None,
            hits: Vec::new(),
            tracks: Vec::new(),
            gallery_scroll: Anim::default(),
            settings_scroll: Anim::default(),
            page_in: Anim { value: 1.0, target: 1.0 },
            gallery_max: 0.0,
            settings_max: 0.0,
            switches: [Anim::default(); 8],
            autostart: host.autostart(),
            hotkeys_ok: true,
            update: None,
            updating: Updating::Idle,
            tab_x: Anim::default(),
            tab_w: Anim::default(),
            tab_widths: Vec::new(),
            latency: 0.0,
            glyphs: voices::glyphs(48),
            raw: Box::new([0.0; SCOPE_LEN]),
            voiced: Box::new([0.0; SCOPE_LEN]),
            dirty: true,
            quiet_frames: 0,
            shown: false,
            watching: false,
            still: false,
            host,
        };
        lang::choose(&app.settings.lang, &app.host.language());
        let s = &app.settings;
        let switched = [s.enabled, s.gate, s.monitor, s.exclusive, s.tray, app.autostart, s.hotkeys, s.updates];
        for (i, on) in switched.into_iter().enumerate() {
            let v = if on { 1.0 } else { 0.0 };
            app.switches[i] = Anim { value: v, target: v };
        }
        if !app.favorites().is_empty() {
            app.shelf = Shelf::Favorites;
        }
        app.refresh_devices();
        // Without the cable nothing hears Goby yet: open on the steps that set it up.
        if app.cable.is_none() {
            app.page = Page::Programs;
        }
        app.name_cable_once();
        app.sync_apps();
        app.apply();
        app.restart();
        app
    }

    fn save(&self) {
        self.host.save(&self.settings);
    }

    /// The client area changed size, in DIPs.
    pub fn resize(&mut self, w: f32, h: f32) {
        self.lay = Layout::new(w, h);
        self.dirty = true;
    }

    /// Re-reads the endpoints. The devices chosen in Settings win whenever they are plugged
    /// in; while one is away the one in use stays, or else Windows' default mic and the cable.
    fn refresh_devices(&mut self) {
        let open_in = self.inputs.get(self.input).map(|e| e.id.clone()).unwrap_or_default();
        let open_out = self.outputs.get(self.output).map(|e| e.id.clone()).unwrap_or_default();
        self.inputs = self.host.devices(true);
        self.outputs = self.host.devices(false);
        self.cable = self.host.cable();
        let default_mic = self.host.default_device(true).map(|d| d.id).unwrap_or_default();
        let cable = self.cable.as_ref().map(|(play, _)| play.id.clone()).unwrap_or_default();
        self.input = first_present(&self.inputs, &[&self.settings.input, &open_in, &default_mic]);
        self.output = first_present(&self.outputs, &[&self.settings.output, &open_out, &cable]);
    }

    /// Unplugging the chosen mic (or output) moves Goby to another one; this moves it back
    /// once the chosen one is plugged in again. Only lists endpoints while that is pending.
    fn rejoin_chosen_devices(&mut self) {
        if self.retry_at.is_some() {
            return;
        }
        let running = self.host.running();
        let in_use = |chosen: &str, open: Option<&Endpoint>| running && open.is_some_and(|e| e.id == chosen);
        let away = |chosen: &str, open: Option<&Endpoint>| !chosen.is_empty() && !in_use(chosen, open);
        let mic_away = away(&self.settings.input, self.inputs.get(self.input));
        let out_away = away(&self.settings.output, self.outputs.get(self.output));
        let host = &mut self.host;
        let mut back = |capture: bool, id: &str| host.devices(capture).iter().any(|e| e.id == id);
        if (mic_away && back(true, &self.settings.input)) || (out_away && back(false, &self.settings.output)) {
            self.refresh_devices();
            self.restart();
        }
    }

    /// A cable installed while Goby runs is picked up within two seconds: the voice goes
    /// to it and it gets its Goby name, with no restart.
    fn look_for_cable(&mut self) {
        let Some((play, _)) = self.host.cable() else { return };
        self.refresh_devices();
        if let Some(i) = self.outputs.iter().position(|e| e.id == play.id) {
            self.output = i;
        }
        self.restart();
        self.name_cable_once();
        self.cable_found = Some(self.host.now());
        self.dirty = true;
    }

    /// First run with an unnamed cable: rename it to Goby once, behind one UAC prompt,
    /// so CS2, Discord and OBS list "Goby Mic" instead of "CABLE Output".
    fn name_cable_once(&mut self) {
        let Some((play, record)) = &self.cable else { return };
        if play.name.starts_with("Goby") || self.settings.cable_named_tried {
            return;
        }
        self.settings.cable_named_tried = true;
        self.host.save(&self.settings);
        self.host.name_cable(play, record);
        self.recheck_at = Some(self.host.now() + Duration::from_secs(4));
    }

    /// Re-reads which programs use the mic and pins the chosen ones to Goby (or hands
    /// them back). Windows keeps the choice per program, so it outlives this process.
    fn sync_apps(&mut self) {
        self.apps_at = self.host.now();
        if self.cable.is_none() {
            self.look_for_cable();
        }
        let recent = self.host.recent_apps();
        for app in recent.iter().filter(|a| !a.path.is_empty()) {
            if !self.exe_paths.iter().any(|(e, _)| *e == app.exe) {
                self.exe_paths.push((app.exe.clone(), app.path.clone()));
            }
        }
        let offered: Vec<MicApp> =
            recent.into_iter().filter(|a| !self.settings.routed.contains(&a.exe)).take(15).collect();
        // Without the cable there is nothing to pin to, and "" would match every program.
        let Some(record) = self.cable.as_ref().map(|(_, r)| r.id.clone()) else {
            let states = vec![AppState::Waiting; self.settings.routed.len()];
            if offered != self.mic_apps || states != self.app_state {
                self.dirty = true;
            }
            (self.mic_apps, self.app_state) = (offered, states);
            return;
        };
        let before = self.settings.unroute.len();
        let s = &mut self.settings;
        let pinned = self.host.pin(&record, &s.routed, &mut s.unroute, &mut self.exe_paths, self.watching);
        if let Some((live, elsewhere)) = pinned.live
            && (live != self.app_live || elsewhere != self.app_elsewhere)
        {
            (self.app_live, self.app_elsewhere) = (live, elsewhere);
            self.dirty = true;
        }
        if self.settings.unroute.len() != before {
            self.save();
        }
        if offered != self.mic_apps || pinned.states != self.app_state {
            self.dirty = true;
        }
        self.mic_apps = offered;
        self.app_state = pinned.states;
    }

    /// Adds a program to the list (or does nothing if it is already there).
    fn route(&mut self, exe: String) {
        let s = &mut self.settings;
        s.unroute.retain(|e| *e != exe);
        if !s.routed.contains(&exe) {
            s.routed.push(exe);
        }
        self.save();
        self.host.repin();
        self.sync_apps();
    }

    fn apply(&mut self) {
        let s = &mut self.settings;
        s.voice = voices::ALL[self.voice].id.to_string();
        self.host.voice(self.voice, s.semitones, s.volume);
        self.host.mic(s.mic_gain_db, s.gate.then_some(s.gate_db), s.dry_mix, s.enabled);
        self.dirty = true;
    }

    fn restart(&mut self) {
        // Release the devices before opening them again.
        self.host.stop();
        self.retry_at = None;
        let (Some(mic), Some(out)) = (self.inputs.get(self.input), self.outputs.get(self.output)) else {
            self.healthy = false;
            self.status =
                if self.inputs.is_empty() { tr("status.no_mic").into() } else { tr("status.no_output").into() };
            return;
        };
        // A device opened in place of an unplugged one is not a choice: the setting keeps
        // naming the one to go back to. Only a first run with nothing chosen adopts these.
        if self.settings.input.is_empty() {
            self.settings.input = mic.id.clone();
        }
        if self.settings.output.is_empty() {
            self.settings.output = out.id.clone();
        }
        let mut targets = vec![out.id.clone()];
        if self.settings.monitor
            && let Some(d) = self.host.default_device(false).filter(|d| d.id != out.id)
        {
            targets.push(d.id);
        }
        match self.host.start(&mic.id, self.settings.exclusive, &targets) {
            Ok(()) => {
                self.healthy = true;
                self.status.clear();
            }
            Err(e) => {
                self.healthy = false;
                self.status = e;
                self.retry_at = Some(self.host.now() + Duration::from_secs(2));
            }
        }
        self.save();
        self.dirty = true;
    }

    fn knob(&self, k: Knob) -> f32 {
        let s = &self.settings;
        match k {
            Knob::Tone => s.semitones,
            Knob::Volume => s.volume,
            Knob::Mix => s.dry_mix,
            Knob::Gain => s.mic_gain_db,
            Knob::Gate => s.gate_db,
        }
    }

    fn set_knob(&mut self, k: Knob, v: f32) {
        let (lo, hi, step, _) = k.range();
        let v = clamp((v / step).round() * step, lo, hi);
        let s = &mut self.settings;
        match k {
            Knob::Tone => s.semitones = v,
            Knob::Volume => s.volume = v,
            Knob::Mix => s.dry_mix = v,
            Knob::Gain => s.mic_gain_db = v,
            Knob::Gate => s.gate_db = v,
        }
        self.apply();
    }

    fn readout(&self, k: Knob) -> String {
        let v = self.knob(k);
        let signed =
            |v: f32| if v > 0.0 { format!("+{}", write_decimal(v)) } else { write_decimal(v).replace('-', "−") };
        match k {
            Knob::Tone => trf("unit.semitones", &[("value", &signed(v))]),
            Knob::Volume | Knob::Mix => format!("{} %", write_decimal((v * 100.0).round())),
            Knob::Gain | Knob::Gate => trf("unit.db", &[("value", &signed(v))]),
        }
    }

    /// How far an animation moves towards its target in one tick.
    fn rate(&self, rate: f32) -> f32 {
        if self.still { 1.0 } else { rate }
    }

    /// Runs once a frame while the window can be seen, and slowly otherwise.
    pub fn tick(&mut self) {
        // The moment the Programs page comes into view, ask what each program records from
        // rather than show what was true when it was last seen.
        let watching = self.shown && self.page == Page::Programs;
        let looked = watching && !self.watching;
        self.watching = watching;
        if looked {
            self.sync_apps();
        }
        let now = self.host.now();
        if self.host.failed() && self.retry_at.is_none() {
            self.host.stop();
            self.healthy = false;
            self.status = tr("status.lost").into();
            self.retry_at = Some(now + Duration::from_secs(2));
            self.dirty = true;
        }
        if now.saturating_sub(self.apps_at) >= Duration::from_secs(2) {
            self.sync_apps();
            self.rejoin_chosen_devices();
        }
        if self.recheck_at.is_some_and(|t| now >= t) {
            self.recheck_at = None;
            self.refresh_devices();
            self.dirty = true;
        }
        if self.retry_at.is_some_and(|t| now >= t) {
            self.refresh_devices();
            self.restart();
        }
        let s = &self.settings;
        let on = [s.enabled, s.gate, s.monitor, s.exclusive, s.tray, self.autostart, s.hotkeys, s.updates];
        let (slide, ease, rise) = (self.rate(0.35), self.rate(0.3), self.rate(0.35));
        let mut moving = false;
        for (a, on) in self.switches.iter_mut().zip(on) {
            a.target = if on { 1.0 } else { 0.0 };
            moving |= a.step(slide);
        }
        moving |= self.tab_x.step(ease) | self.tab_w.step(ease);
        moving |= self.gallery_scroll.step(ease) | self.settings_scroll.step(ease) | self.page_in.step(rise);
        self.dirty |= moving;
        if !self.shown {
            return;
        }

        let measured = self.host.latency_ms();
        self.latency = if self.latency == 0.0 { measured } else { self.latency + (measured - self.latency) * 0.05 };

        // Only redraw the scope while there is something to see.
        if self.host.scope(&mut self.raw, &mut self.voiced) {
            let loud = |x: &[f32]| x.iter().any(|s| s.abs() > 0.003);
            let active = loud(&self.raw[SCOPE_LEN - 960..]) || loud(&self.voiced[SCOPE_LEN - 960..]);
            self.quiet_frames = if active { 0 } else { self.quiet_frames + 1 };
            if self.quiet_frames < 3 {
                self.dirty = true;
            }
        }
    }
}

/// Oscilloscope trace locked onto a rising zero crossing so the wave stands still,
/// squashed through tanh so loud voices stay inside the card.
fn trace(samples: &[f32; SCOPE_LEN], area: R) -> Vec<(f32, f32)> {
    const SPAN: usize = 768;
    const POINTS: usize = 192;
    let start =
        (1..SCOPE_LEN - SPAN).rev().find(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0).unwrap_or(SCOPE_LEN - SPAN);
    let mid = area.t + area.h() / 2.0;
    (0..=POINTS)
        .map(|k| {
            let s = samples[(start + k * SPAN / POINTS).min(SCOPE_LEN - 1)];
            (area.l + area.w() * k as f32 / POINTS as f32, mid - (s * 4.0).tanh() * area.h() * 0.48)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_stays_inside_its_card() {
        let area = R::new(10.0, 20.0, 300.0, 100.0);
        let mut loud = Box::new([0.0f32; SCOPE_LEN]);
        for (i, s) in loud.iter_mut().enumerate() {
            *s = ((i as f32) * 0.05).sin() * 5.0;
        }
        for samples in [&*loud, &[0.0f32; SCOPE_LEN]] {
            let pts = trace(samples, area);
            assert!(pts.iter().all(|(x, y)| area.has((*x, *y)) || (*x - area.r).abs() < 1e-3), "trace left the card");
        }
    }

    #[test]
    fn knob_ranges_hold_their_reset_values() {
        for k in [Knob::Tone, Knob::Volume, Knob::Mix, Knob::Gain, Knob::Gate] {
            let (lo, hi, step, reset) = k.range();
            assert!(lo < hi && step > 0.0 && (lo..=hi).contains(&reset), "{k:?}");
            let steps = (hi - lo) / step;
            assert!((steps - steps.round()).abs() < 1e-3, "{k:?} range is not a whole number of steps");
        }
    }

    #[test]
    fn an_unplugged_mic_is_taken_back_when_it_returns() {
        let ep = |id: &str| Endpoint { id: id.into(), name: id.into() };
        let (usb, webcam) = (ep("usb"), ep("webcam"));
        // The order refresh_devices asks in: the chosen mic, the open one, Windows' default.
        let pick = |list: &[Endpoint], open: &str| first_present(list, &["usb", open, "webcam"]);
        let both = [webcam.clone(), usb.clone()];
        assert_eq!(both[pick(&both, "")].id, "usb", "first start");
        let unplugged = [webcam.clone()];
        assert_eq!(unplugged[pick(&unplugged, "usb")].id, "webcam", "unplugged: Windows' default stands in");
        // Plugged back in while the webcam is open: the old order (open one first) stayed on it.
        assert_eq!(both[pick(&both, "webcam")].id, "usb", "replugged");
        assert_eq!(first_present(&both, &["", "gone"]), 0);
        assert_eq!(first_present(&[], &["usb"]), 0);
    }

    #[test]
    fn known_programs_get_their_own_icons() {
        assert_eq!(app_glyph("discord.exe"), glyph::CHAT);
        assert_eq!(app_glyph("obs64.exe"), glyph::VIDEO);
        assert_eq!(app_glyph("cs2.exe"), glyph::GAME);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn layout_fills_any_window_without_overlap() {
        for (w, h) in [(MIN_W, MIN_H), (WIDTH, HEIGHT), (1600.0, 1000.0), (3000.0, 1600.0), (400.0, 300.0)] {
            let l = Layout::new(w, h);
            let (w, h) = (w.max(MIN_W), h.max(MIN_H));
            for r in [l.scope, l.quick, l.tabs, l.gallery] {
                assert!(
                    r.l >= RAIL_W && r.r <= w - 23.0 && r.b <= h - 15.0,
                    "{w}x{h}: off the window or under the rail"
                );
            }
            assert!(l.scope.b < l.quick.t && l.quick.b < l.tabs.t && l.tabs.b < l.gallery.t, "{w}x{h}: rows overlap");
            assert!(l.gallery.h() >= 2.0 * TILE_H, "{w}x{h}: fewer than two rows of voices");
            assert!(
                l.nav_voices.b < l.nav_programs.t && l.nav_programs.b < l.nav_settings.t && l.nav_settings.b <= h,
                "{w}x{h}: rail items overlap"
            );
            let tile_w = (l.gallery.w() - GUTTER - TILE_GAP * (l.cols - 1) as f32) / l.cols as f32;
            assert!(tile_w >= TILE_MIN_W - 0.01 || l.cols == 2, "{w}x{h}: tiles too narrow ({tile_w})");
            assert!(l.pane.w() >= 300.0 && l.pane.w() <= SETTINGS_W, "{w}x{h}: settings column {}", l.pane.w());
            assert!(l.pane.l >= RAIL_W && l.title.b <= l.pane.t, "{w}x{h}: settings column misplaced");
        }
        assert!(Layout::new(1600.0, 1000.0).cols > Layout::new(MIN_W, MIN_H).cols, "wider windows show more columns");
    }
}

#[cfg(test)]
mod seam_tests;
