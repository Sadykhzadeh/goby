//! The window, laid out like Windows 11 Settings: Mica backdrop, cards, the system accent,
//! Segoe UI Variable and native menus. Plain Win32 underneath; drawn by `gfx`.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCSTR, PCWSTR, PWSTR, Result, w};

use crate::apps;
use crate::engine::{self, Endpoint, Engine, SCOPE_LEN, Shared};
use crate::gfx::{Align, Font, Gfx, R, glyph};
use crate::hotkeys;
use crate::icon;
use crate::install;
use crate::lang;
use crate::policy;
use crate::settings::{Preset, Settings};
use crate::theme::{Theme, argb, with_alpha};
use crate::tray;
use crate::voices::{self, Category};

// Layout, in DIPs.
/// Default and smallest client sizes, in DIPs.
const WIDTH: f32 = 1000.0;
const HEIGHT: f32 = 720.0;
const MIN_W: f32 = 820.0;
const MIN_H: f32 = 560.0;
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

const WS_EX_NOREDIRECTIONBITMAP: WINDOW_EX_STYLE = WINDOW_EX_STYLE(0x0020_0000);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Knob {
    Tone,
    Volume,
    Mix,
    Gain,
    Gate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Switch {
    Effect,
    Gate,
    Monitor,
    Exclusive,
    Tray,
    Autostart,
    Hotkeys,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pick {
    Mic,
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Pane {
    Gallery,
    Settings,
}

/// What a line of the tray icon's menu does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TrayAct {
    None,
    Open,
    Pick(Fav),
    Effect,
    Quit,
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

/// A native popup menu, prepared by the window and shown by `show` with no borrow of the
/// window state alive. Choices are reported by key (device id or exe name), not position.
struct Popup {
    pick: Option<Pick>,
    labels: Vec<String>,
    keys: Vec<String>,
    checked: Option<usize>,
    /// Label of the last item and title of the file dialog it opens.
    browse: Option<(String, String)>,
    at: POINT,
}

impl Popup {
    fn show(&self, hwnd: HWND) -> Option<String> {
        const BROWSE: usize = 10_000;
        let chosen = unsafe {
            let menu = CreatePopupMenu().ok()?;
            for (i, label) in self.labels.iter().enumerate() {
                let flags = if self.checked == Some(i) { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(label.as_str()));
            }
            if let Some((label, _)) = &self.browse {
                if !self.labels.is_empty() {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                let _ = AppendMenuW(menu, MF_STRING, BROWSE, &HSTRING::from(label.as_str()));
            }
            let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN | TPM_TOPALIGN;
            let id = TrackPopupMenuEx(menu, flags.0, self.at.x, self.at.y, hwnd, None);
            let _ = DestroyMenu(menu);
            id.0 as usize
        };
        match chosen {
            0 => None,
            BROWSE => browse_exe(hwnd, &self.browse.as_ref()?.1),
            i => self.keys.get(i - 1).cloned(),
        }
    }
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

struct App {
    hwnd: HWND,
    lay: Layout,
    tab_scroll: f32,
    tab_max: f32,
    gfx: Option<Gfx>,
    theme: Theme,
    mica: bool,
    ru: bool,
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

    shared: Arc<Shared>,
    engine: Option<Engine>,
    status: String,
    healthy: bool,
    retry_at: Option<Instant>,
    /// The virtual cable (play side, record side), if installed.
    cable: Option<(Endpoint, Endpoint)>,
    /// Recent microphone users not chosen yet (offered by "Add"), and the state of each
    /// chosen program, in `settings.routed` order.
    mic_apps: Vec<apps::MicApp>,
    app_state: Vec<AppState>,
    /// Each chosen program has the mic open right now, in `settings.routed` order.
    app_live: Vec<bool>,
    apps_at: Instant,
    /// Processes already pinned to Goby, so each is asked only once, and the cable path
    /// they were pinned to: a different cable starts the cache over.
    pinned: Vec<u32>,
    pinned_to: String,
    /// Re-read device names a moment after the elevated rename was started.
    recheck_at: Option<Instant>,

    mouse: (f32, f32),
    hover: Hit,
    drag: Option<Drag>,
    hits: Vec<(R, Hit)>,
    tracks: Vec<(Knob, R)>,
    gallery_scroll: Anim,
    settings_scroll: Anim,
    gallery_max: f32,
    settings_max: f32,
    switches: [Anim; 7],
    /// Windows starts Goby at sign-in; kept in the registry, read once here.
    autostart: bool,
    /// Every hotkey registered; false when another program holds one.
    hotkeys_ok: bool,
    /// The notification-area icon is up, and the tip it shows.
    tray_icon: bool,
    tray_tip: String,
    tab_x: Anim,
    tab_w: Anim,
    tab_widths: Vec<f32>,
    latency: f32,
    glyphs: Vec<Vec<f32>>,
    raw: Box<[f32; SCOPE_LEN]>,
    voiced: Box<[f32; SCOPE_LEN]>,
    dirty: bool,
    quiet_frames: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AppState {
    /// Running and recording from Goby.
    OnGoby,
    /// Not running; it will be pinned when it starts.
    Waiting,
    /// Running, but Windows will only pin it once it has opened audio.
    Pending,
}

/// A Segoe Fluent icon for well-known programs; a game pad for everything else.
fn app_glyph(exe: &str) -> u16 {
    match exe {
        "discord.exe" | "telegram.exe" | "teams.exe" | "ms-teams.exe" | "zoom.exe" | "skype.exe" => glyph::CHAT,
        "chrome.exe" | "msedge.exe" | "firefox.exe" | "opera.exe" | "brave.exe" | "steamwebhelper.exe" => glyph::GLOBE,
        "obs64.exe" | "obs32.exe" => glyph::VIDEO,
        _ => glyph::GAME,
    }
}

fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

#[repr(C)]
struct OpenFileName {
    struct_size: u32,
    owner: HWND,
    instance: HINSTANCE,
    filter: PCWSTR,
    custom_filter: PWSTR,
    max_custom_filter: u32,
    filter_index: u32,
    file: PWSTR,
    max_file: u32,
    file_title: PWSTR,
    max_file_title: u32,
    initial_dir: PCWSTR,
    title: PCWSTR,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_ext: PCWSTR,
    custom_data: LPARAM,
    hook: *mut std::ffi::c_void,
    template_name: PCWSTR,
    reserved: *mut std::ffi::c_void,
    reserved2: u32,
    flags_ex: u32,
}

#[link(name = "comdlg32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> BOOL;
}

/// The standard Open dialog, filtered to programs; returns the lower-case exe name.
fn browse_exe(owner: HWND, title: &str) -> Option<String> {
    let mut file = [0u16; 1024];
    let filter: Vec<u16> = "Programs (*.exe)\0*.exe\0\0".encode_utf16().collect();
    let title = HSTRING::from(title);
    let mut ofn = OpenFileName {
        struct_size: size_of::<OpenFileName>() as u32,
        owner,
        instance: HINSTANCE::default(),
        filter: PCWSTR(filter.as_ptr()),
        custom_filter: PWSTR::null(),
        max_custom_filter: 0,
        filter_index: 1,
        file: PWSTR(file.as_mut_ptr()),
        max_file: file.len() as u32,
        file_title: PWSTR::null(),
        max_file_title: 0,
        initial_dir: PCWSTR::null(),
        title: PCWSTR(title.as_ptr()),
        // OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
        flags: 0x0008_0000 | 0x1000 | 0x0800 | 0x0008,
        file_offset: 0,
        file_extension: 0,
        default_ext: PCWSTR::null(),
        custom_data: LPARAM(0),
        hook: std::ptr::null_mut(),
        template_name: PCWSTR::null(),
        reserved: std::ptr::null_mut(),
        reserved2: 0,
        flags_ex: 0,
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let len = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..len]);
    let name = path.rsplit('\\').next()?.to_ascii_lowercase();
    name.ends_with(".exe").then_some(name)
}

/// Lets Win32 menus follow the dark theme. uxtheme exports these by ordinal only; they have
/// been stable since Windows 10 1903 and are what Windows' own apps use for dark menus.
fn allow_dark_menus(hwnd: HWND) {
    unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_preferred_app_mode(1); // AllowDark: follow the system setting
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(133 as *const u8)) {
            let allow_for_window: extern "system" fn(HWND, BOOL) -> BOOL = std::mem::transmute(f);
            let _ = allow_for_window(hwnd, true.into());
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let flush_menu_themes: extern "system" fn() = std::mem::transmute(f);
            flush_menu_themes();
        }
    }
}

impl App {
    fn new(hwnd: HWND) -> App {
        let settings = Settings::load();
        let ru = lang::russian();
        let mut app = App {
            hwnd,
            lay: Layout::new(WIDTH, HEIGHT),
            tab_scroll: 0.0,
            tab_max: 0.0,
            gfx: None,
            theme: Theme::system(),
            mica: false,
            ru,
            dpi: unsafe { GetDpiForWindow(hwnd) } as f32,
            voice: voices::index_of(&settings.voice).unwrap_or(1),
            settings,
            inputs: Vec::new(),
            outputs: Vec::new(),
            input: 0,
            output: 0,
            shelf: Shelf::All,
            advanced: false,
            page: Page::Voices,
            shared: Arc::new(Shared::default()),
            engine: None,
            status: String::new(),
            healthy: true,
            retry_at: None,
            cable: None,
            mic_apps: Vec::new(),
            app_state: Vec::new(),
            app_live: Vec::new(),
            apps_at: Instant::now(),
            pinned: Vec::new(),
            pinned_to: String::new(),
            recheck_at: None,
            mouse: (-1.0, -1.0),
            hover: Hit::None,
            drag: None,
            hits: Vec::new(),
            tracks: Vec::new(),
            gallery_scroll: Anim::default(),
            settings_scroll: Anim::default(),
            gallery_max: 0.0,
            settings_max: 0.0,
            switches: [Anim::default(); 7],
            autostart: install::starts_with_windows(),
            hotkeys_ok: true,
            tray_icon: false,
            tray_tip: String::new(),
            tab_x: Anim::default(),
            tab_w: Anim::default(),
            tab_widths: Vec::new(),
            latency: 0.0,
            glyphs: voices::ALL.iter().map(|v| voices::glyph(v, 48)).collect(),
            raw: Box::new([0.0; SCOPE_LEN]),
            voiced: Box::new([0.0; SCOPE_LEN]),
            dirty: true,
            quiet_frames: 0,
        };
        let s = &app.settings;
        let switched = [s.enabled, s.gate, s.monitor, s.exclusive, s.tray, app.autostart, s.hotkeys];
        for (i, on) in switched.into_iter().enumerate() {
            let v = if on { 1.0 } else { 0.0 };
            app.switches[i] = Anim { value: v, target: v };
        }
        if !app.favorites().is_empty() {
            app.shelf = Shelf::Favorites;
        }
        app.refresh_devices();
        app.name_cable_once();
        app.sync_apps();
        app.apply();
        app.restart();
        app
    }

    fn t(&self, ru: &'static str, en: &'static str) -> &'static str {
        if self.ru { ru } else { en }
    }

    // ---------- tray ----------

    /// What the icon says on hover: the voice that is on, or that the effect is off.
    fn tip(&self) -> String {
        let voice = voices::ALL.get(self.voice).map_or("", |v| v.name(self.ru));
        match self.settings.enabled {
            true => format!("Goby: {voice}"),
            false => format!("Goby: {}", self.t("эффект выключен", "effect off")),
        }
    }

    fn add_tray_icon(&mut self) {
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, self.dpi as u32) };
        let Some(icon) = make_icon(size, icon::RGB) else { return };
        self.tray_tip = self.tip();
        self.tray_icon = tray::add(self.hwnd, icon, &self.tray_tip);
    }

    fn show(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, if IsIconic(self.hwnd).as_bool() { SW_RESTORE } else { SW_SHOW });
            let _ = SetForegroundWindow(self.hwnd);
        }
        self.dirty = true;
    }

    /// Closing the window: it goes to the tray while the icon is there to bring it back.
    /// True when it should be destroyed instead, which frees `self`, so the caller does it.
    fn close(&mut self) -> bool {
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
    fn tray_menu(&self) -> Vec<(tray::Item, TrayAct)> {
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
    fn tray_act(&mut self, act: TrayAct) -> bool {
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

    // ---------- favourites ----------

    /// Saved voices first, then the starred ones, each in the order it was added.
    fn favorites(&self) -> Vec<Fav> {
        let saved = (0..self.settings.presets.len())
            .filter(|p| voices::index_of(&self.settings.presets[*p].voice).is_some())
            .map(Fav::Preset);
        let starred = self.settings.favorites.iter().filter_map(|id| voices::index_of(id)).map(Fav::Voice);
        saved.chain(starred).collect()
    }

    /// The saved voice that is set exactly as it is playing now, if any.
    fn active_preset(&self) -> Option<usize> {
        let s = &self.settings;
        let id = voices::ALL[self.voice].id;
        s.presets.iter().position(|p| p.matches(id, s.semitones, s.dry_mix, s.volume))
    }

    fn current(&self) -> Fav {
        self.active_preset().map_or(Fav::Voice(self.voice), Fav::Preset)
    }

    /// The voice a tile plays, for its waveform.
    fn voice_of(&self, f: Fav) -> usize {
        match f {
            Fav::Voice(i) => i,
            Fav::Preset(p) => self.settings.presets.get(p).and_then(|p| voices::index_of(&p.voice)).unwrap_or(0),
        }
    }

    fn label(&self, f: Fav) -> String {
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
    fn choose(&mut self, f: Fav, wake: bool) {
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
    fn tuned(&self) -> bool {
        let s = &self.settings;
        s.semitones != 0.0 || s.dry_mix != 0.0 || (s.volume - 1.0).abs() > 0.005
    }

    fn save_preset(&mut self) {
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

    fn drop_preset(&mut self, p: usize) {
        if p < self.settings.presets.len() {
            self.settings.presets.remove(p);
            self.settings.save();
            self.hover = Hit::None;
            self.dirty = true;
        }
    }

    fn is_favorite(&self, i: usize) -> bool {
        self.settings.favorites.iter().any(|id| id == voices::ALL[i].id)
    }

    fn toggle_favorite(&mut self, i: usize) {
        let id = voices::ALL[i].id;
        match self.is_favorite(i) {
            true => self.settings.favorites.retain(|f| f != id),
            false => self.settings.favorites.push(id.to_string()),
        }
        self.settings.save();
        self.dirty = true;
    }

    // ---------- hotkeys ----------

    fn use_hotkeys(&mut self) {
        hotkeys::unregister(self.hwnd);
        self.hotkeys_ok = !self.settings.hotkeys || hotkeys::register(self.hwnd);
    }

    fn hotkey(&mut self, action: hotkeys::Action) {
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

    // ---------- state ----------

    fn refresh_devices(&mut self) {
        let keep_in = self.inputs.get(self.input).map(|e| e.id.clone()).unwrap_or(self.settings.input.clone());
        let keep_out = self.outputs.get(self.output).map(|e| e.id.clone()).unwrap_or(self.settings.output.clone());
        self.inputs = engine::list(true);
        self.outputs = engine::list(false);
        self.cable = engine::cable();
        let pick = |list: &[Endpoint], id: &str| list.iter().position(|e| e.id == id);
        self.input = pick(&self.inputs, &keep_in)
            .or_else(|| engine::default_endpoint(true).and_then(|d| pick(&self.inputs, &d.id)))
            .unwrap_or(0);
        self.output = pick(&self.outputs, &keep_out)
            .or_else(|| self.cable.as_ref().and_then(|(play, _)| pick(&self.outputs, &play.id)))
            .unwrap_or(0);
    }

    /// First run with an unnamed cable: rename it to Goby once, behind one UAC prompt,
    /// so CS2, Discord and OBS list "Goby Mic" instead of "CABLE Output".
    fn name_cable_once(&mut self) {
        let Some((play, record)) = &self.cable else { return };
        if play.name.starts_with("Goby") || self.settings.cable_named_tried {
            return;
        }
        self.settings.cable_named_tried = true;
        self.settings.save();
        let exe = std::env::current_exe().unwrap_or_default();
        let args = format!("--name-cable \"{}\" \"{}\"", play.id, record.id);
        let (exe, args) = (HSTRING::from(exe.as_os_str()), HSTRING::from(args));
        unsafe {
            ShellExecuteW(
                Some(self.hwnd),
                w!("runas"),
                PCWSTR(exe.as_ptr()),
                PCWSTR(args.as_ptr()),
                PCWSTR::null(),
                SW_HIDE,
            );
        }
        self.recheck_at = Some(Instant::now() + Duration::from_secs(4));
    }

    /// Re-reads which programs use the mic and pins the chosen ones to Goby (or hands
    /// them back). Windows keeps the choice per program, so it outlives this process.
    fn sync_apps(&mut self) {
        self.apps_at = Instant::now();
        let recent = apps::recent();
        let live: Vec<bool> =
            self.settings.routed.iter().map(|exe| recent.iter().any(|a| a.exe == *exe && a.active)).collect();
        if live != self.app_live {
            self.app_live = live;
            self.dirty = true;
        }
        let offered: Vec<apps::MicApp> =
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
        let ours = policy::capture_interface_path(&record);
        if ours != self.pinned_to {
            self.pinned.clear();
            self.pinned_to = ours.clone();
        }
        let snapshot = apps::processes();
        self.pinned.retain(|pid| snapshot.iter().any(|(_, p)| p == pid));

        let mut states = Vec::with_capacity(self.settings.routed.len());
        for exe in &self.settings.routed {
            let pids = apps::pids_of(&snapshot, exe);
            let mut pinned = pids.iter().any(|p| self.pinned.contains(p));
            // One success covers the whole program; Windows stores the choice per program.
            for pid in pids.iter().filter(|_| !pinned) {
                let already = policy::app_route(*pid).is_ok_and(|now| now.eq_ignore_ascii_case(&ours));
                if already || policy::route_app(*pid, Some(&record)).is_ok() {
                    pinned = true;
                    self.pinned.push(*pid);
                    break;
                }
            }
            states.push(match (pids.is_empty(), pinned) {
                (true, _) => AppState::Waiting,
                (false, true) => AppState::OnGoby,
                (false, false) => AppState::Pending,
            });
        }

        // Hand removed programs back to the default mic once they run; a program that is
        // no longer on Goby has nothing to undo.
        let before = self.settings.unroute.len();
        let mut unroute = std::mem::take(&mut self.settings.unroute);
        unroute.retain(|exe| {
            for pid in apps::pids_of(&snapshot, exe) {
                match policy::app_route(pid) {
                    Ok(now) if !now.eq_ignore_ascii_case(&ours) => return false,
                    Ok(_) if policy::route_app(pid, None).is_ok() => return false,
                    _ => {}
                }
            }
            true
        });
        self.settings.unroute = unroute;
        if self.settings.unroute.len() != before {
            self.settings.save();
        }

        if offered != self.mic_apps || states != self.app_state {
            self.dirty = true;
        }
        self.mic_apps = offered;
        self.app_state = states;
    }

    /// Adds a program to the list (or does nothing if it is already there).
    fn route(&mut self, exe: String) {
        let s = &mut self.settings;
        s.unroute.retain(|e| *e != exe);
        if !s.routed.contains(&exe) {
            s.routed.push(exe);
        }
        self.settings.save();
        self.pinned.clear();
        self.sync_apps();
    }

    /// The "Add a program" menu: recent microphone users, plus "Other program…".
    fn app_popup(&self) -> Option<Popup> {
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
    fn device_popup(&mut self, pick: Pick) -> Option<Popup> {
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

    fn screen_point(&self, x: f32, y: f32) -> POINT {
        let scale = self.dpi / 96.0;
        let mut p = POINT { x: (x * scale) as i32, y: (y * scale) as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut p);
        }
        p
    }

    /// Applies what was chosen in a popup, by key: the lists may have changed meanwhile.
    fn popup_done(&mut self, pick: Option<Pick>, key: String) {
        match pick {
            None => self.route(key),
            Some(pick) => {
                let list = if pick == Pick::Mic { &self.inputs } else { &self.outputs };
                if let Some(i) = list.iter().position(|e| e.id == key) {
                    match pick {
                        Pick::Mic => self.input = i,
                        Pick::Out => self.output = i,
                    }
                    self.restart();
                }
            }
        }
        self.dirty = true;
    }

    fn apply(&mut self) {
        let s = &mut self.settings;
        s.voice = voices::ALL[self.voice].id.to_string();
        self.shared.set(self.voice, s.semitones, s.volume);
        self.shared.set_mic(s.mic_gain_db, s.gate.then_some(s.gate_db), s.dry_mix, s.enabled);
        self.dirty = true;
    }

    fn restart(&mut self) {
        // Release the devices before opening them again.
        self.engine = None;
        self.retry_at = None;
        self.shared.failed.store(false, Relaxed);
        let (Some(mic), Some(out)) = (self.inputs.get(self.input), self.outputs.get(self.output)) else {
            self.healthy = false;
            self.status = if self.inputs.is_empty() {
                self.t("Микрофон не найден", "No microphone found").into()
            } else {
                self.t("Нет устройств вывода", "No output devices").into()
            };
            return;
        };
        self.settings.input = mic.id.clone();
        self.settings.output = out.id.clone();
        let mut targets = vec![out.id.clone()];
        if self.settings.monitor
            && let Some(d) = engine::default_endpoint(false).filter(|d| d.id != out.id)
        {
            targets.push(d.id);
        }
        match Engine::start(&mic.id, self.settings.exclusive, &targets, self.shared.clone()) {
            Ok(e) => {
                self.healthy = true;
                self.status.clear();
                self.engine = Some(e);
            }
            Err(e) => {
                self.healthy = false;
                self.status = e;
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        self.settings.save();
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
        let v = ((v / step).round() * step).clamp(lo, hi);
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
        let signed = |v: f32| if v > 0.0 { format!("+{v}") } else { format!("{v}").replace('-', "−") };
        match k {
            Knob::Tone => format!("{} {}", signed(v), self.t("пт", "st")),
            Knob::Volume | Knob::Mix => format!("{} %", (v * 100.0).round()),
            Knob::Gain | Knob::Gate => format!("{} {}", signed(v), self.t("дБ", "dB")),
        }
    }

    fn filtered(&self) -> Vec<Fav> {
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
    fn is_chosen(&self, f: Fav) -> bool {
        match f {
            Fav::Preset(p) => self.active_preset() == Some(p),
            Fav::Voice(i) => i == self.voice && (self.shelf != Shelf::Favorites || self.active_preset().is_none()),
        }
    }

    fn tick(&mut self) {
        if self.shared.failed.load(Relaxed) && self.retry_at.is_none() {
            self.engine = None;
            self.healthy = false;
            self.status = self.t("Устройство пропало, переподключаюсь…", "Device lost, reconnecting…").into();
            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            self.dirty = true;
        }
        if self.apps_at.elapsed() >= Duration::from_secs(2) {
            self.sync_apps();
        }
        if self.recheck_at.is_some_and(|t| Instant::now() >= t) {
            self.recheck_at = None;
            self.refresh_devices();
            self.dirty = true;
        }
        if self.retry_at.is_some_and(|t| Instant::now() >= t) {
            self.refresh_devices();
            self.restart();
        }
        let s = &self.settings;
        let on = [s.enabled, s.gate, s.monitor, s.exclusive, s.tray, self.autostart, s.hotkeys];
        let mut moving = false;
        for (a, on) in self.switches.iter_mut().zip(on) {
            a.target = if on { 1.0 } else { 0.0 };
            moving |= a.step(0.35);
        }
        if self.tray_icon {
            let tip = self.tip();
            if tip != self.tray_tip {
                tray::set_tip(self.hwnd, &tip);
                self.tray_tip = tip;
            }
        }
        moving |= self.tab_x.step(0.3) | self.tab_w.step(0.3);
        moving |= self.gallery_scroll.step(0.3) | self.settings_scroll.step(0.3);
        self.dirty |= moving;

        let measured = self.shared.latency_ms();
        self.latency = if self.latency == 0.0 { measured } else { self.latency + (measured - self.latency) * 0.05 };

        // Only redraw the scope while there is something to see.
        self.shared.scope(&mut self.raw, &mut self.voiced);
        let loud = |x: &[f32]| x.iter().any(|s| s.abs() > 0.003);
        let active = loud(&self.raw[SCOPE_LEN - 960..]) || loud(&self.voiced[SCOPE_LEN - 960..]);
        self.quiet_frames = if active { 0 } else { self.quiet_frames + 1 };
        if self.quiet_frames < 3 {
            self.dirty = true;
        }
    }

    // ---------- input ----------

    fn hit_at(&self, p: (f32, f32)) -> Hit {
        self.hits.iter().rev().find(|(r, _)| r.has(p)).map_or(Hit::None, |(_, h)| *h)
    }

    fn slider_from_x(&mut self, k: Knob, x: f32) {
        let Some((_, track)) = self.tracks.iter().find(|(kk, _)| *kk == k) else { return };
        let (lo, hi, _, _) = k.range();
        let t = ((x - track.l) / track.w()).clamp(0.0, 1.0);
        self.set_knob(k, lo + t * (hi - lo));
    }

    /// Handles a click. Menus are returned rather than shown: see `Popup`.
    fn press(&mut self, double: bool) -> Option<Popup> {
        let hit = self.hit_at(self.mouse);
        match hit {
            Hit::Tile(i) => self.choose(Fav::Voice(i), false),
            Hit::Preset(p) => self.choose(Fav::Preset(p), false),
            Hit::DropPreset(p) => self.drop_preset(p),
            Hit::SavePreset => self.save_preset(),
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
            Hit::CableSite => unsafe {
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

    fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.settings_scroll = Anim::default();
            self.hits.clear();
            self.hover = Hit::None;
        }
        self.dirty = true;
    }

    fn moved(&mut self) {
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

    fn release(&mut self) {
        if self.drag.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
            self.settings.save();
            self.dirty = true;
        }
    }

    fn wheel(&mut self, notches: f32) {
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
    fn key(&mut self, vk: u16) -> bool {
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

    // ---------- drawing ----------

    fn paint(&mut self) -> Result<()> {
        if self.gfx.is_none() {
            let mut rc = RECT::default();
            unsafe { GetClientRect(self.hwnd, &mut rc)? };
            self.gfx = Some(Gfx::new(self.hwnd, rc.right as u32, rc.bottom as u32, self.dpi)?);
        }
        let g = self.gfx.take().unwrap();
        let background = if self.mica { D2D1_COLOR_F::default() } else { self.theme.solid_background() };
        g.begin(background);
        let drawn = self.draw(&g);
        let presented = g.end();
        self.gfx = Some(g);
        drawn?;
        if presented.is_err() {
            // Device lost (driver update, GPU reset): rebuild on the next frame.
            self.gfx = None;
        }
        self.dirty = false;
        Ok(())
    }

    fn draw(&mut self, g: &Gfx) -> Result<()> {
        self.hits.clear();
        self.tracks.clear();
        self.draw_rail(g)?;
        match self.page {
            Page::Voices => {
                self.draw_scope(g)?;
                self.draw_quick(g)?;
                self.draw_tabs(g)?;
                self.draw_gallery(g)?;
            }
            Page::Programs => self.draw_programs(g)?,
            Page::Settings => self.draw_settings(g)?,
        }
        self.draw_rail_tip(g)?;
        Ok(())
    }

    fn rail(&self) -> [(Page, R, u16, &'static str); 3] {
        [
            (Page::Voices, self.lay.nav_voices, glyph::VOICES, self.t("Голоса", "Voices")),
            (Page::Programs, self.lay.nav_programs, glyph::APPS, self.t("Программы", "Programs")),
            (Page::Settings, self.lay.nav_settings, glyph::SETTINGS, self.t("Настройки", "Settings")),
        ]
    }

    /// The compact NavigationView: an icon per page, the current one marked by an accent pill.
    fn draw_rail(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        for (page, r, icon, _) in self.rail() {
            let hit = Hit::Nav(page);
            if self.page == page || self.hover == hit {
                g.fill(r, 4.0, th.control_hover());
            }
            if self.page == page {
                g.fill(R::new(r.l, r.t + 12.0, 3.0, 16.0), 1.5, th.accent());
            }
            g.icon(icon, Font::Icon16, r, th.text())?;
            self.hits.push((r, hit));
        }
        Ok(())
    }

    /// The name of a rail item under the pointer, as the compact rail's tooltip shows it.
    fn draw_rail_tip(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let Some((_, r, _, label)) = self.rail().into_iter().find(|(p, ..)| self.hover == Hit::Nav(*p)) else {
            return Ok(());
        };
        let w = g.measure(label, Font::Body)? + 20.0;
        let tip = R::new(r.r + 6.0, r.t + 6.0, w, 28.0);
        g.fill(tip, 4.0, th.control_solid());
        g.stroke(tip, 4.0, th.strong_stroke(), 1.0);
        g.text(label, Font::Body, tip, Align::Center, th.text())?;
        Ok(())
    }

    /// Which programs hear Goby, and whether each one does right now.
    fn draw_programs(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let title = self.lay.title;
        g.text(self.t("Программы", "Programs"), Font::Title, title, Align::Start, th.text())?;
        let scroll = self.settings_scroll.value.min(self.settings_max);
        let (x, w) = (self.lay.pane.l, self.lay.pane.w() - GUTTER);
        let mut y = self.lay.pane.t - scroll;
        g.clip(self.lay.pane);

        let note = self.t(
            "Выбранные программы слышат Goby вместо микрофона. Остальные слышат ваш обычный микрофон.",
            "The programs chosen here hear Goby instead of your microphone. Everything else hears your own mic.",
        );
        g.text(note, Font::Body, R::new(x + 2.0, y, w, 22.0), Align::Start, th.text_secondary())?;
        y += 36.0;

        match &self.cable {
            None => {
                let none = self.t(
                    "Установите бесплатный VB-Cable и перезапустите Goby",
                    "Install the free VB-Cable, then restart Goby",
                );
                let title = self.t("Нет виртуального кабеля", "No virtual cable");
                self.row_card(g, &mut y, Hit::CableSite, glyph::GAME, title, none)?;
            }
            Some((_, record)) => {
                let ready = self.t("Через него Goby попадает в программы", "Goby reaches the programs through it");
                let name = short(&record.name).to_string();
                self.row_card(g, &mut y, Hit::None, glyph::MIC, &name, ready)?;
            }
        }
        y += 20.0;

        let section = |g: &Gfx, y: &mut f32, title: &str| -> Result<()> {
            g.text(title, Font::Strong, R::new(x + 2.0, *y, w, 20.0), Align::Start, th.text())?;
            *y += 28.0;
            Ok(())
        };
        section(g, &mut y, self.t("Слышат Goby", "Hear Goby"))?;
        for i in 0..self.settings.routed.len() {
            let exe = self.settings.routed[i].clone();
            let talking = self.app_live.get(i).copied().unwrap_or(false);
            let desc = match (self.app_state.get(i).copied().unwrap_or(AppState::Waiting), talking) {
                (AppState::OnGoby, true) => self.t("Слышит Goby прямо сейчас", "Hearing Goby right now"),
                (AppState::OnGoby, false) => self.t("Запущена и слышит Goby", "Running, hears Goby"),
                (AppState::Waiting, _) => self.t("Услышит Goby при запуске", "Hears Goby once it starts"),
                (AppState::Pending, _) => self.t("Услышит, когда откроет микрофон", "Hears Goby once it opens the mic"),
            };
            self.row_card(g, &mut y, Hit::Remove(i), app_glyph(&exe), &apps::display_name(&exe), desc)?;
        }
        let hint = if self.settings.routed.is_empty() {
            self.t("Например CS2 или Discord", "For example CS2 or Discord")
        } else {
            self.t("Из списка или любой exe-файл", "From the list or any program file")
        };
        self.row_card(g, &mut y, Hit::AddApp, glyph::ADD, self.t("Добавить программу", "Add a program"), hint)?;

        if !self.mic_apps.is_empty() {
            y += 20.0;
            section(g, &mut y, self.t("Недавно открывали микрофон", "Recently used the microphone"))?;
            for i in 0..self.mic_apps.len().min(6) {
                let app = &self.mic_apps[i];
                let (name, exe) = (app.name.clone(), app.exe.clone());
                let desc = match app.active {
                    true => self.t("Слушает микрофон сейчас · добавить", "Listening now · add"),
                    false => self.t("Добавить, чтобы слышала Goby", "Add it to hear Goby"),
                };
                self.row_card(g, &mut y, Hit::Offer(i), app_glyph(&exe), &name, desc)?;
            }
        }

        g.unclip();
        let content = y + scroll - self.lay.pane.t;
        self.settings_max = (content - self.lay.pane.h()).max(0.0);
        self.settings_scroll.target = self.settings_scroll.target.min(self.settings_max);
        self.scrollbar(g, self.lay.pane, scroll, self.settings_max, Pane::Settings);
        Ok(())
    }

    /// What gets changed while playing: pitch, volume and hearing yourself, side by side.
    fn draw_quick(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let r = self.lay.quick;
        self.card(g, r, 8.0, false);
        let w = r.w() / 3.0;
        for (i, (k, icon, title)) in [
            (Knob::Tone, glyph::TONE, self.t("Тон", "Pitch")),
            (Knob::Volume, glyph::VOLUME, self.t("Громкость", "Volume")),
        ]
        .into_iter()
        .enumerate()
        {
            let s = R::new(r.l + w * i as f32, r.t, w, r.h());
            g.icon(icon, Font::Icon16, R::new(s.l + 16.0, s.t + 12.0, 20.0, 24.0), th.text())?;
            g.text(title, Font::Body, R::new(s.l + 48.0, s.t + 12.0, s.w() - 140.0, 24.0), Align::Start, th.text())?;
            let value = R::new(s.r - 96.0, s.t + 12.0, 80.0, 24.0);
            g.text(&self.readout(k), Font::Caption, value, Align::End, th.text_secondary())?;
            self.slider(g, k, R::new(s.l + 26.0, s.t + 48.0, s.w() - 52.0, 24.0));
            g.fill(R::new(s.r - 0.5, s.t + 16.0, 1.0, s.h() - 32.0), 0.0, th.card_stroke());
        }
        let s = R::new(r.l + w * 2.0, r.t, w, r.h());
        g.icon(glyph::HEADPHONE, Font::Icon16, R::new(s.l + 16.0, s.t + 30.0, 20.0, 24.0), th.text())?;
        let text_w = s.w() - 48.0 - 72.0;
        let hear = self.t("Слышать себя", "Hear myself");
        g.text(hear, Font::Body, R::new(s.l + 48.0, s.t + 20.0, text_w, 22.0), Align::Start, th.text())?;
        let mon = self.t("В ваших наушниках", "In your headphones");
        g.text(mon, Font::Caption, R::new(s.l + 48.0, s.t + 42.0, text_w, 18.0), Align::Start, th.text_secondary())?;
        self.toggle(g, s.r - 60.0, s.t + 32.0, Switch::Monitor);
        self.hits.push((s.inset(4.0), Hit::Toggle(Switch::Monitor)));
        Ok(())
    }

    fn card(&self, g: &Gfx, r: R, radius: f32, hovered: bool) {
        let th = &self.theme;
        g.fill(r, radius, if hovered { th.control_hover() } else { th.card() });
        g.stroke(r, radius, th.card_stroke(), 1.0);
    }

    fn toggle(&self, g: &Gfx, x: f32, y: f32, which: Switch) {
        let t = self.switches[which as usize].value;
        self.toggle_at(g, x, y, t, self.hover == Hit::Toggle(which));
    }

    /// WinUI ToggleSwitch: 40x20 track, 12 px knob that slides and recolours.
    fn toggle_at(&self, g: &Gfx, x: f32, y: f32, t: f32, hovered: bool) {
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
    fn slider(&mut self, g: &Gfx, k: Knob, track: R) {
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

    fn draw_scope(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let r = self.lay.scope;
        self.card(g, r, 8.0, false);
        let def = &voices::ALL[self.voice];
        g.text(
            &self.label(self.current()),
            Font::Subtitle,
            R::new(r.l + 20.0, r.t + 12.0, 380.0, 28.0),
            Align::Start,
            th.text(),
        )?;
        // With nobody listening the voice goes nowhere; say so where it is seen.
        let unheard = self.healthy && (self.cable.is_none() || self.settings.routed.is_empty());
        let (line, color) = if !self.healthy {
            (self.status.clone(), argb(if th.dark { 0xFFFF99A4 } else { 0xFFC42B1C }))
        } else if self.cable.is_none() {
            let none = self.t("Нет виртуального кабеля: откройте «Программы»", "No virtual cable: open Programs");
            (none.into(), th.accent())
        } else if self.settings.routed.is_empty() {
            let none =
                self.t("Пока никто не слышит Goby: выберите программы", "Nobody hears Goby yet: pick the programs");
            (none.into(), th.accent())
        } else if !self.settings.enabled {
            let off = self.t("Эффект выключен: в игру идёт ваш голос", "Effect off: your own voice goes through");
            (off.into(), th.text_secondary())
        } else {
            let lat = format!("{} {} {}", self.t("задержка", "latency"), self.latency.round(), self.t("мс", "ms"));
            (format!("{} · {}", def.category.name(self.ru), lat), th.text_secondary())
        };
        let line_r = R::new(r.l + 20.0, r.t + 40.0, 420.0, 18.0);
        g.text(&line, Font::Caption, line_r, Align::Start, color)?;
        if unheard {
            self.hits.push((line_r, Hit::Nav(Page::Programs)));
        }

        // A voice tuned away from the catalog can be kept; once kept it says where it went.
        if self.active_preset().is_some() {
            let saved = self.t("В избранном", "In Favorites");
            let w = g.measure(saved, Font::Caption)? + 22.0;
            let at = R::new(r.r - 176.0 - w, r.t + 14.0, w, 24.0);
            g.icon(glyph::STAR_FILLED, Font::Icon12, R::new(at.l, at.t, 16.0, 24.0), th.accent())?;
            g.text(saved, Font::Caption, R { l: at.l + 22.0, ..at }, Align::Start, th.text_secondary())?;
        } else if self.tuned() {
            let save = self.t("Сохранить как мой голос", "Save as my voice");
            let w = g.measure(save, Font::Body)? + 48.0;
            let button = R::new(r.r - 176.0 - w, r.t + 10.0, w, 32.0);
            let hovered = self.hover == Hit::SavePreset;
            g.fill(button, 4.0, if hovered { th.control_hover() } else { th.control_alt() });
            g.stroke(button, 4.0, th.control_stroke(), 1.0);
            g.icon(glyph::STAR, Font::Icon12, R::new(button.l + 10.0, button.t, 16.0, 32.0), th.text())?;
            g.text(save, Font::Body, R { l: button.l + 34.0, ..button }, Align::Start, th.text())?;
            self.hits.push((button, Hit::SavePreset));
        }

        let label = self.t("Эффект", "Effect");
        g.text(label, Font::Body, R::new(r.r - 160.0, r.t + 14.0, 88.0, 24.0), Align::End, th.text())?;
        self.toggle(g, r.r - 60.0, r.t + 16.0, Switch::Effect);
        self.hits.push((R::new(r.r - 170.0, r.t + 8.0, 156.0, 36.0), Hit::Toggle(Switch::Effect)));

        let area = R::new(r.l + 20.0, r.t + 64.0, r.w() - 40.0, r.h() - 76.0);
        g.line(&trace(&self.raw, area), th.text_tertiary(), 1.2)?;
        g.line(&trace(&self.voiced, area), th.accent(), 2.0)?;
        Ok(())
    }

    fn draw_tabs(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let mut items: Vec<(Shelf, &'static str)> =
            vec![(Shelf::All, self.t("Все", "All")), (Shelf::Favorites, self.t("Избранное", "Favorites"))];
        items.extend(Category::ALL.iter().map(|c| (Shelf::Group(*c), c.name(self.ru))));
        if self.tab_widths.len() != items.len() {
            self.tab_widths = items.iter().map(|(_, s)| g.measure(s, Font::Strong).unwrap_or(60.0)).collect();
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
            g.text(label, if selected { Font::Strong } else { Font::Body }, r, Align::Center, color)?;
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
        Ok(())
    }

    fn draw_gallery(&mut self, g: &Gfx) -> Result<()> {
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
            g.line(&pts, if selected { th.accent() } else { th.text_secondary() }, 1.4)?;
            let name = R::new(r.l + 14.0, r.t + 44.0, r.w() - 28.0 - 24.0, 22.0);
            g.text(&self.label(f), Font::Body, name, Align::Start, th.text())?;
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
                g.icon(icon, Font::Icon12, button, color)?;
                if (hovered || on_button) && visible.has((button.l + 14.0, button.t + 14.0)) {
                    self.hits.push((button, corner));
                }
            }
        }
        if list.is_empty() && self.shelf == Shelf::Favorites {
            let (l, t, w) = (self.lay.gallery.l + 2.0, self.lay.gallery.t + 8.0, self.lay.gallery.w() - GUTTER);
            let first = self.t(
                "Отметьте голос звёздочкой или правой кнопкой мыши",
                "Star a voice, or right-click it, to keep it here",
            );
            g.text(first, Font::Strong, R::new(l, t, w, 22.0), Align::Start, th.text())?;
            let second = self.t(
                "Избранное есть и в меню значка в трее, а Ctrl+Alt+PgUp/PgDn листают только его",
                "Favorites are in the tray menu too, and Ctrl+Alt+PgUp/PgDn step through them only",
            );
            g.text(second, Font::Body, R::new(l, t + 26.0, w, 22.0), Align::Start, th.text_secondary())?;
        }
        g.unclip();
        self.scrollbar(g, self.lay.gallery, scroll, self.gallery_max, Pane::Gallery);
        Ok(())
    }

    /// Thin overlay scrollbar, wider while hovered or dragged, like WinUI's.
    fn scrollbar(&mut self, g: &Gfx, view: R, scroll: f32, max: f32, pane: Pane) {
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

    fn draw_settings(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let title = self.lay.title;
        g.text(self.t("Настройки", "Settings"), Font::Title, title, Align::Start, th.text())?;

        let scroll = self.settings_scroll.value.min(self.settings_max);
        let (x, w) = (self.lay.pane.l, self.lay.pane.w() - GUTTER);
        let mut y = self.lay.pane.t - scroll;
        g.clip(self.lay.pane);

        let section = |g: &Gfx, y: &mut f32, title: &str| -> Result<()> {
            g.text(title, Font::Strong, R::new(x + 2.0, *y, w, 20.0), Align::Start, th.text())?;
            *y += 28.0;
            Ok(())
        };

        section(g, &mut y, self.t("Микрофон", "Microphone"))?;
        let mic = self.inputs.get(self.input).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Mic), glyph::MIC, self.t("Устройство", "Device"), &mic)?;
        let gate = self.t("Тишина между фразами", "Silence between phrases");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Gate), glyph::GATE, self.t("Шумовой гейт", "Noise gate"), gate)?;
        // Rarely touched once set: folded away like an expander in Windows Settings.
        let more = self.t("Дополнительно", "Advanced");
        let more_desc = self.t("Задержка, усиление, порог гейта", "Latency, gain, gate threshold");
        self.row_card(g, &mut y, Hit::Advanced, glyph::MORE, more, more_desc)?;
        if self.advanced {
            let fast = self.t("Низкая задержка", "Low latency");
            let mode = match (self.settings.exclusive, self.shared.mic_exclusive.load(Relaxed)) {
                (true, true) => self.t("Микрофон занят только Goby", "Only Goby uses the mic"),
                (true, false) => self.t("Микрофон не разрешил", "The mic did not allow it"),
                _ => self.t("Обычный режим, +10 мс", "Shared mode, +10 ms"),
            };
            self.row_card(g, &mut y, Hit::Toggle(Switch::Exclusive), glyph::SPEED, fast, mode)?;
            self.slider_card(g, &mut y, Knob::Gain, glyph::VOLUME, self.t("Усиление", "Gain"))?;
            self.slider_card(g, &mut y, Knob::Gate, glyph::GATE, self.t("Порог гейта", "Gate threshold"))?;
        }
        y += 20.0;

        section(g, &mut y, self.t("Вывод", "Output"))?;
        let out = self.outputs.get(self.output).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Out), glyph::SPEAKERS, self.t("Куда отдавать", "Send to"), &out)?;
        self.slider_card(g, &mut y, Knob::Mix, glyph::MIX, self.t("Свой голос в миксе", "Original voice in mix"))?;
        y += 20.0;

        section(g, &mut y, self.t("Общие", "General"))?;
        let tray = self.t("Работать в трее", "Keep running in the tray");
        let tray_desc = self.t("Голос работает и после закрытия", "The voice stays on when closed");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Tray), glyph::MINIMIZE, tray, tray_desc)?;
        let start = self.t("Запускать вместе с Windows", "Start with Windows");
        let start_desc = self.t("Сразу в трей, без окна", "Straight to the tray, no window");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Autostart), glyph::POWER, start, start_desc)?;
        let keys = self.t("Горячие клавиши", "Hotkeys");
        let keys_desc = match self.hotkeys_ok {
            true => self.t("Ctrl+Alt+G эффект, PgUp/PgDn голос", "Ctrl+Alt+G effect, PgUp/PgDn voice"),
            false => self.t("Некоторые клавиши заняты", "Some keys are taken elsewhere"),
        };
        self.row_card(g, &mut y, Hit::Toggle(Switch::Hotkeys), glyph::KEYBOARD, keys, keys_desc)?;

        y += 20.0;
        let about = format!("Goby {}", env!("CARGO_PKG_VERSION"));
        self.row_card(g, &mut y, Hit::About, glyph::INFO, &about, "Azer Sadykhzadeh · azer.one")?;

        g.unclip();
        let content = y + scroll - self.lay.pane.t;
        self.settings_max = (content - self.lay.pane.h()).max(0.0);
        self.settings_scroll.target = self.settings_scroll.target.min(self.settings_max);
        self.scrollbar(g, self.lay.pane, scroll, self.settings_max, Pane::Settings);
        Ok(())
    }

    /// SettingsCard with an icon, a title and a value, and a slider underneath.
    fn slider_card(&mut self, g: &Gfx, y: &mut f32, k: Knob, icon: u16, title: &str) -> Result<()> {
        let th = self.theme;
        let r = R::new(self.lay.pane.l, *y, self.lay.pane.w() - GUTTER, 84.0);
        if r.intersects(&self.lay.pane) {
            self.card(g, r, 4.0, false);
            g.icon(icon, Font::Icon16, R::new(r.l + 16.0, r.t + 12.0, 20.0, 24.0), th.text())?;
            g.text(title, Font::Body, R::new(r.l + 52.0, r.t + 12.0, r.w() - 140.0, 24.0), Align::Start, th.text())?;
            let value = R::new(r.r - 96.0, r.t + 12.0, 80.0, 24.0);
            g.text(&self.readout(k), Font::Caption, value, Align::End, th.text_secondary())?;
            self.slider(g, k, R::new(r.l + 26.0, r.t + 48.0, r.w() - 52.0, 24.0));
        }
        *y += 84.0 + CARD_GAP;
        Ok(())
    }

    /// SettingsCard row: icon, title, description, and a toggle or chevron on the right.
    fn row_card(&mut self, g: &Gfx, y: &mut f32, hit: Hit, icon: u16, title: &str, desc: &str) -> Result<()> {
        let th = self.theme;
        let r = R::new(self.lay.pane.l, *y, self.lay.pane.w() - GUTTER, 68.0);
        if r.intersects(&self.lay.pane) {
            self.card(g, r, 4.0, self.hover == hit && !matches!(hit, Hit::Remove(_)));
            g.icon(icon, Font::Icon16, R::new(r.l + 16.0, r.t + 22.0, 20.0, 24.0), th.text())?;
            let text_w = r.w() - 52.0 - 64.0;
            g.text(title, Font::Body, R::new(r.l + 52.0, r.t + 13.0, text_w, 22.0), Align::Start, th.text())?;
            g.text(
                desc,
                Font::Caption,
                R::new(r.l + 52.0, r.t + 35.0, text_w, 18.0),
                Align::Start,
                th.text_secondary(),
            )?;
            let side = R::new(r.r - 36.0, r.t + 24.0, 20.0, 20.0);
            match hit {
                Hit::Toggle(s) => self.toggle(g, r.r - 56.0, r.t + 24.0, s),
                Hit::Remove(_) => {
                    // A small close button; the card itself does nothing.
                    let b = R::new(r.r - 48.0, r.t + 18.0, 32.0, 32.0);
                    if self.hover == hit {
                        g.fill(b, 4.0, th.control_hover());
                    }
                    g.icon(glyph::REMOVE, Font::Icon12, b, th.text_secondary())?;
                }
                Hit::AddApp => g.icon(glyph::CHEVRON_DOWN, Font::Icon12, side, th.text_secondary())?,
                Hit::Offer(_) => g.icon(glyph::ADD, Font::Icon12, side, th.text_secondary())?,
                Hit::Advanced => {
                    let chevron = if self.advanced { glyph::CHEVRON_UP } else { glyph::CHEVRON_DOWN };
                    g.icon(chevron, Font::Icon12, side, th.text_secondary())?
                }
                Hit::Pick(_) => g.icon(glyph::CHEVRON_DOWN, Font::Icon12, side, th.text_secondary())?,
                Hit::None => {}
                _ => g.icon(glyph::OPEN, Font::Icon12, side, th.text_secondary())?,
            }
            let target = if matches!(hit, Hit::Remove(_)) { R::new(r.r - 48.0, r.t + 18.0, 32.0, 32.0) } else { r };
            let visible = R { t: target.t.max(self.lay.pane.t), b: target.b.min(self.lay.pane.b), ..target };
            self.hits.push((visible, hit));
        }
        *y += 68.0 + CARD_GAP;
        Ok(())
    }

    /// True while a fullscreen window of another app (a game) covers our monitor.
    fn covered(&self) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() || fg == self.hwnd {
                return false;
            }
            let mut wr = RECT::default();
            if GetWindowRect(fg, &mut wr).is_err() {
                return false;
            }
            let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
            if monitor != MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) {
                return false;
            }
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(monitor, &mut info).as_bool() && info.rcMonitor == wr
        }
    }

    fn apply_frame(&mut self) {
        unsafe {
            let dark = BOOL::from(self.theme.dark);
            let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
            let backdrop = DWMSBT_MAINWINDOW;
            let size = size_of_val(&backdrop) as u32;
            self.mica =
                DwmSetWindowAttribute(self.hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, size).is_ok();
            if self.mica {
                let all = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
                self.mica = DwmExtendFrameIntoClientArea(self.hwnd, &all).is_ok();
            }
            for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
                if let Some(icon) = make_icon(size, icon::RGB) {
                    let icon = LPARAM(icon.0 as isize);
                    let old = SendMessageW(self.hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(icon));
                    if old.0 != 0 {
                        let _ = DestroyIcon(HICON(old.0 as _));
                    }
                }
            }
        }
        allow_dark_menus(self.hwnd);
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

/// The window icon, identical to the executable's so the title bar matches the taskbar.
fn make_icon(size: i32, rgb: u32) -> Option<HICON> {
    let px = icon::premultiplied(size as usize, rgb);
    unsafe {
        let bitmap = CreateBitmap(size, size, 1, 32, Some(px.as_ptr() as _));
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: bitmap };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// A saved voice's name: the voice and what was changed, e.g. "Demon +3 · 20% own".
fn preset_label(name: &str, p: &Preset, ru: bool) -> String {
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

/// The message Explorer broadcasts when the taskbar comes back after a restart.
fn taskbar_created() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe fn app_of(hwnd: HWND) -> Option<&'static mut App> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    unsafe { p.as_mut() }
}

fn point(lp: LPARAM, dpi: f32) -> (f32, f32) {
    let x = (lp.0 & 0xffff) as i16 as f32;
    let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
    (x * 96.0 / dpi, y * 96.0 / dpi)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(app) = app_of(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_PAINT => {
                if let Err(e) = app.paint() {
                    app.status = e.message();
                    app.healthy = false;
                }
                let _ = ValidateRect(Some(hwnd), None);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                app.tick();
                if app.dirty && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() && !app.covered() {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                app.mouse = point(lp, app.dpi);
                app.moved();
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                app.hover = Hit::None;
                app.dirty = true;
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                app.mouse = point(lp, app.dpi);
                if let Some(popup) = app.press(msg == WM_LBUTTONDBLCLK) {
                    // The menu runs a modal loop that sends timers and paints back here, so
                    // `app` is not used across it; it is fetched again afterwards.
                    let chosen = popup.show(hwnd);
                    let Some(app) = app_of(hwnd) else { return LRESULT(0) };
                    if let Some(key) = chosen {
                        app.popup_done(popup.pick, key);
                    }
                    app.hover = app.hit_at(app.mouse);
                    app.dirty = true;
                    return LRESULT(0);
                }
                app.hover = app.hit_at(app.mouse);
                app.dirty = true;
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                app.mouse = point(lp, app.dpi);
                if let Hit::Tile(i) | Hit::Star(i) = app.hit_at(app.mouse) {
                    app.toggle_favorite(i);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                app.release();
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let mut p = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                app.mouse = (p.x as f32 * 96.0 / app.dpi, p.y as f32 * 96.0 / app.dpi);
                app.wheel(((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0);
                LRESULT(0)
            }
            WM_KEYDOWN if app.key(wp.0 as u16) => LRESULT(0),
            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
                let (w, h) = window_size(MIN_W, MIN_H, app.dpi as u32, style);
                info.ptMinTrackSize = POINT { x: w, y: h };
                LRESULT(0)
            }
            WM_SIZE => {
                let (w, h) = ((lp.0 & 0xffff) as u32, ((lp.0 >> 16) & 0xffff) as u32);
                if w > 0 && h > 0 {
                    let scale = app.dpi / 96.0;
                    app.lay = Layout::new(w as f32 / scale, h as f32 / scale);
                }
                if wp.0 as u32 == SIZE_MINIMIZED {
                    // Give the GPU memory back while we sit in the taskbar.
                    app.gfx = None;
                } else if let Some(g) = &mut app.gfx
                    && g.resize(w, h, app.dpi).is_err()
                {
                    app.gfx = None;
                }
                app.dirty = true;
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.dpi = (wp.0 & 0xffff) as f32;
                app.gfx = None;
                let r = *(lp.0 as *const RECT);
                let (w, h) = (r.right - r.left, r.bottom - r.top);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // Light/dark or accent changed in Windows settings: follow it live.
                let theme = Theme::system();
                if theme != app.theme {
                    app.theme = theme;
                    app.apply_frame();
                    app.dirty = true;
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ACTIVATE => {
                app.dirty = true;
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CLOSE => {
                if app.close() {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_HOTKEY => {
                if let Some(action) = hotkeys::action(wp.0) {
                    app.hotkey(action);
                }
                LRESULT(0)
            }
            tray::WM_QUIT_GOBY => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            tray::WM_TRAY => {
                match tray::event(lp) {
                    tray::SELECT | tray::KEY_SELECT => app.show(),
                    WM_CONTEXTMENU => {
                        let (items, acts): (Vec<_>, Vec<_>) = app.tray_menu().into_iter().unzip();
                        let chosen = tray::menu(hwnd, tray::anchor(wp), &items).and_then(|i| acts.get(i).copied());
                        let Some(app) = app_of(hwnd) else { return LRESULT(0) };
                        if chosen.is_some_and(|act| app.tray_act(act)) {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            m if m == taskbar_created() => {
                // Explorer restarted and forgot every icon.
                app.add_tray_icon();
                LRESULT(0)
            }
            WM_DESTROY => {
                tray::remove(hwnd);
                hotkeys::unregister(hwnd);
                app.settings.save();
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn window_size(w: f32, h: f32, dpi: u32, style: WINDOW_STYLE) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: (w * dpi as f32 / 96.0).round() as i32,
        bottom: (h * dpi as f32 / 96.0).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Opens the window, or with `hidden` starts with only the tray icon.
pub fn run(hidden: bool) -> Result<()> {
    unsafe {
        // A second Goby would open a second engine into the same cable: bring the first one
        // forward instead.
        let _guard = CreateMutexW(None, true, w!("Goby.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // A start at sign-in finds Goby already up and leaves it where it is.
            if hidden {
                return Ok(());
            }
            if let Ok(hwnd) = FindWindowW(w!("GobyWindow"), PCWSTR::null()) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return Ok(());
        }
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: w!("GobyWindow"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let style = WS_OVERLAPPEDWINDOW;
        let (w, h) = window_size(WIDTH, HEIGHT, GetDpiForSystem(), style);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            w!("GobyWindow"),
            w!("Goby"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        // Fit the monitor the window landed on: its DPI, and its work area on small laptops.
        let dpi = GetDpiForWindow(hwnd);
        let (mut w, mut h) = window_size(WIDTH, HEIGHT, dpi, style);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            let work = info.rcWork;
            let (min_w, min_h) = window_size(MIN_W, MIN_H, dpi, style);
            w = w.min(work.right - work.left).max(min_w);
            h = h.min(work.bottom - work.top).max(min_h);
            let x = work.left + (work.right - work.left - w).max(0) / 2;
            let y = work.top + (work.bottom - work.top - h).max(0) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
        }

        let mut app = Box::new(App::new(hwnd));
        app.apply_frame();
        app.add_tray_icon();
        app.use_hotkeys();
        let hidden = hidden && app.tray_icon;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        SetTimer(Some(hwnd), 1, 33, None);
        if !hidden {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
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
