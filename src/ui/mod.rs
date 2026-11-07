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

mod background;
mod controls;
mod input;
mod library;
mod pages;
mod updates;

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
    Updates,
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
    /// Each chosen program records from Goby right now, in `settings.routed` order.
    app_live: Vec<bool>,
    /// The real microphone a chosen program records from instead, when it picks its own.
    app_elsewhere: Vec<Option<String>>,
    /// Where each program seen so far lives (lower-case exe name to full path), for icons.
    exe_paths: Vec<(String, String)>,
    apps_at: Instant,
    /// Processes already pinned to Goby, so each is asked only once, and the cable path
    /// they were pinned to: a different cable starts the cache over.
    pinned: Vec<u32>,
    pinned_to: String,
    /// The VB-Cable page was opened from the setup steps this session.
    cable_opened: bool,
    /// When a cable installed while Goby was open was found, to say so for a while.
    cable_found: Option<Instant>,
    /// Re-read device names a moment after the elevated rename was started.
    recheck_at: Option<Instant>,

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
    /// A newer verified release, and where installing it has got to.
    update: Option<crate::update::Manifest>,
    updating: updates::Updating,
    inbox: Arc<std::sync::Mutex<updates::Inbox>>,
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
    /// The window can be seen: shown, not minimised, not under a fullscreen game.
    shown: bool,
    /// The Programs page is on screen, so what each program records from is worth asking.
    watching: bool,
    /// Milliseconds between timer ticks: a frame while shown, much slower otherwise.
    pace: u32,
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
        "nvidia broadcast.exe" => glyph::MIC,
        _ => glyph::GAME,
    }
}

/// The place in `list` of the first of `ids` that is in it, or 0.
fn first_present(list: &[Endpoint], ids: &[&str]) -> usize {
    ids.iter().filter(|id| !id.is_empty()).find_map(|id| list.iter().position(|e| e.id == *id)).unwrap_or(0)
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
            app_elsewhere: Vec::new(),
            exe_paths: Vec::new(),
            apps_at: Instant::now(),
            pinned: Vec::new(),
            pinned_to: String::new(),
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
            autostart: install::starts_with_windows(),
            hotkeys_ok: true,
            update: None,
            updating: updates::Updating::Idle,
            inbox: updates::inbox(),
            tray_icon: false,
            tray_tip: String::new(),
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
            pace: 0,
        };
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

    fn t(&self, ru: &'static str, en: &'static str) -> &'static str {
        if self.ru { ru } else { en }
    }

    /// Re-reads the endpoints. The devices chosen in Settings win whenever they are plugged
    /// in; while one is away the one in use stays, or else Windows' default mic and the cable.
    fn refresh_devices(&mut self) {
        let open_in = self.inputs.get(self.input).map(|e| e.id.clone()).unwrap_or_default();
        let open_out = self.outputs.get(self.output).map(|e| e.id.clone()).unwrap_or_default();
        self.inputs = engine::list(true);
        self.outputs = engine::list(false);
        self.cable = engine::cable();
        let default_mic = engine::default_endpoint(true).map(|d| d.id).unwrap_or_default();
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
        let running = self.engine.is_some();
        let in_use = |chosen: &str, open: Option<&Endpoint>| running && open.is_some_and(|e| e.id == chosen);
        let away = |chosen: &str, open: Option<&Endpoint>| !chosen.is_empty() && !in_use(chosen, open);
        let mic_away = away(&self.settings.input, self.inputs.get(self.input));
        let out_away = away(&self.settings.output, self.outputs.get(self.output));
        let back = |capture: bool, id: &str| engine::list(capture).iter().any(|e| e.id == id);
        if (mic_away && back(true, &self.settings.input)) || (out_away && back(false, &self.settings.output)) {
            self.refresh_devices();
            self.restart();
        }
    }

    /// A cable installed while Goby runs is picked up within two seconds: the voice goes
    /// to it and it gets its Goby name, with no restart.
    fn look_for_cable(&mut self) {
        let Some((play, _)) = engine::cable() else { return };
        self.refresh_devices();
        if let Some(i) = self.outputs.iter().position(|e| e.id == play.id) {
            self.output = i;
        }
        self.restart();
        self.name_cable_once();
        self.cable_found = Some(Instant::now());
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
        if self.cable.is_none() {
            self.look_for_cable();
        }
        let recent = apps::recent();
        for app in recent.iter().filter(|a| !a.path.is_empty()) {
            if !self.exe_paths.iter().any(|(e, _)| *e == app.exe) {
                self.exe_paths.push((app.exe.clone(), app.path.clone()));
            }
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
        // Listing every process takes 4 to 6 ms of kernel time; with no program to pin or
        // hand back, nothing below looks at it.
        let idle = self.settings.routed.is_empty() && self.settings.unroute.is_empty();
        let snapshot = if idle { Vec::new() } else { apps::processes() };
        self.pinned.retain(|pid| snapshot.iter().any(|(_, p)| p == pid));

        // Which microphone each chosen program really records from at this moment: the
        // per-app default only reaches programs that use the default, and one with its own
        // microphone setting is the case worth saying out loud.
        // A chosen program Windows has not listed yet still has an icon on disk.
        for exe in &self.settings.routed {
            if !self.exe_paths.iter().any(|(e, _)| e == exe)
                && let Some(path) = apps::pids_of(&snapshot, exe).into_iter().find_map(apps::exe_path)
            {
                self.exe_paths.push((exe.clone(), path));
            }
        }
        // Asking every microphone for its sessions takes 3 to 5 ms, and only the Programs
        // page shows the answer: skip it while that page cannot be seen.
        if self.watching {
            let sessions = if self.settings.routed.is_empty() { Vec::new() } else { engine::recording() };
            let (mut live, mut elsewhere) = (Vec::new(), Vec::new());
            for exe in &self.settings.routed {
                let pids = apps::pids_of(&snapshot, exe);
                let mine = sessions.iter().filter(|(pid, _)| pids.contains(pid));
                live.push(mine.clone().any(|(_, ep)| ep.id == record));
                elsewhere
                    .push(mine.into_iter().find(|(_, ep)| ep.id != record).map(|(_, ep)| short(&ep.name).to_string()));
            }
            if live != self.app_live || elsewhere != self.app_elsewhere {
                (self.app_live, self.app_elsewhere) = (live, elsewhere);
                self.dirty = true;
            }
        }

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

    fn tick(&mut self) {
        // The moment the Programs page comes into view, ask what each program records from
        // rather than show what was true when it was last seen.
        let watching = self.shown && self.page == Page::Programs;
        let looked = watching && !self.watching;
        self.watching = watching;
        if looked {
            self.sync_apps();
        }
        if self.shared.failed.load(Relaxed) && self.retry_at.is_none() {
            self.engine = None;
            self.healthy = false;
            self.status = self.t("Устройство пропало, переподключаюсь…", "Device lost, reconnecting…").into();
            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            self.dirty = true;
        }
        if self.apps_at.elapsed() >= Duration::from_secs(2) {
            self.sync_apps();
            self.rejoin_chosen_devices();
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
        let on = [s.enabled, s.gate, s.monitor, s.exclusive, s.tray, self.autostart, s.hotkeys, s.updates];
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
        moving |= self.gallery_scroll.step(0.3) | self.settings_scroll.step(0.3) | self.page_in.step(0.35);
        self.dirty |= moving;
        if !self.shown {
            return;
        }

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

    /// Ticks once a frame while the window can be seen. Otherwise only the device retry,
    /// program pinning and the tray tip need the timer, and twice a second is plenty.
    fn pace(&mut self, shown: bool) {
        let ms = if shown { 33 } else { 500 };
        if ms != self.pace {
            self.pace = ms;
            unsafe { SetTimer(Some(self.hwnd), 1, ms, None) };
        }
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

/// The message Explorer broadcasts when the taskbar comes back after a restart.
/// Windows' "Animation effects" switch (Settings > Accessibility > Visual effects).
fn animations_on() -> bool {
    let mut on = BOOL(1);
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut BOOL as _), Default::default())
    };
    ok.is_err() || on.as_bool()
}

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
                app.shown = IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() && !app.covered();
                app.tick();
                if app.dirty && app.shown {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                app.pace(app.shown);
                LRESULT(0)
            }
            // Back from the tray or the taskbar: frames again from the next tick, not the
            // next slow one.
            WM_SHOWWINDOW if wp.0 != 0 => {
                app.pace(true);
                DefWindowProcW(hwnd, msg, wp, lp)
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
                } else {
                    app.pace(true);
                    if let Some(g) = &mut app.gfx
                        && g.resize(w, h, app.dpi).is_err()
                    {
                        app.gfx = None;
                    }
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
                app.pace(true);
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
            updates::WM_UPDATE_CHECKED => {
                app.update_checked();
                LRESULT(0)
            }
            updates::WM_UPDATE_READY => {
                app.update_ready();
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
        app.check_for_update(Duration::from_secs(15));
        let hidden = hidden && app.tray_icon;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        if let Some(app) = app_of(hwnd) {
            app.pace(!hidden);
        }
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
