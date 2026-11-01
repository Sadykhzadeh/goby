//! The window, laid out like Windows 11 Settings: Mica backdrop, cards, the system accent,
//! Segoe UI Variable and native menus. Plain Win32 underneath; drawn by `gfx`.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HRESULT, HSTRING, PCSTR, PCWSTR, Result, w};

use crate::engine::{self, Endpoint, Engine, SCOPE_LEN, Shared};
use crate::gfx::{Align, Font, Gfx, R, glyph};
use crate::policy;
use crate::settings::Settings;
use crate::theme::{Theme, argb, with_alpha};
use crate::voices::{self, Category};

// Layout, in DIPs.
const WIDTH: f32 = 920.0;
const HEIGHT: f32 = 720.0;
const SCOPE: R = R::new(24.0, 16.0, 560.0, 164.0);
const TABS_Y: f32 = 192.0;
const GALLERY: R = R::new(24.0, 236.0, 560.0, 468.0);
const COLS: usize = 4;
const TILE_GAP: f32 = 8.0;
/// Room on the right of a scrolling pane for its scrollbar.
const GUTTER: f32 = 12.0;
const TILE_H: f32 = 76.0;
const PANE: R = R::new(604.0, 16.0, 292.0, 688.0);
const CARD_GAP: f32 = 4.0;

const WM_MOUSELEAVE: u32 = 0x02A3;
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    None,
    Tile(usize),
    Tab(Option<Category>),
    Slider(Knob),
    Toggle(Switch),
    Pick(Pick),
    /// Make the cable the Windows default microphone.
    MakeDefault,
    /// Rename the cable to Goby (elevated).
    NameCable,
    Scrollbar(Pane),
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
    filter: Option<Category>,

    shared: Arc<Shared>,
    engine: Option<Engine>,
    status: String,
    healthy: bool,
    retry_at: Option<Instant>,
    /// The virtual cable (play side, record side), if installed.
    cable: Option<(Endpoint, Endpoint)>,
    cable_is_default: bool,
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
    switches: [Anim; 4],
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

fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

#[repr(C)]
struct Margins {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

#[link(name = "shell32", kind = "raw-dylib")]
unsafe extern "system" {
    fn ShellExecuteW(hwnd: HWND, op: PCWSTR, file: PCWSTR, params: PCWSTR, dir: PCWSTR, show: i32) -> HINSTANCE;
}

#[link(name = "dwmapi", kind = "raw-dylib")]
unsafe extern "system" {
    fn DwmExtendFrameIntoClientArea(hwnd: HWND, margins: *const Margins) -> HRESULT;
}

/// Lets Win32 menus follow the dark theme. uxtheme exports these by ordinal only; they have
/// been stable since Windows 10 1903 and are what Explorer and Notepad use.
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
        let ru = unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x19;
        let mut app = App {
            hwnd,
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
            filter: None,
            shared: Arc::new(Shared::default()),
            engine: None,
            status: String::new(),
            healthy: true,
            retry_at: None,
            cable: None,
            cable_is_default: false,
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
            switches: [Anim::default(); 4],
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
        for (i, on) in [s.enabled, s.gate, s.monitor, s.exclusive].into_iter().enumerate() {
            let v = if on { 1.0 } else { 0.0 };
            app.switches[i] = Anim { value: v, target: v };
        }
        app.refresh_devices();
        app.apply();
        app.restart();
        app
    }

    fn t(&self, ru: &'static str, en: &'static str) -> &'static str {
        if self.ru { ru } else { en }
    }

    // ---------- state ----------

    fn refresh_devices(&mut self) {
        let keep_in = self.inputs.get(self.input).map(|e| e.id.clone()).unwrap_or(self.settings.input.clone());
        let keep_out = self.outputs.get(self.output).map(|e| e.id.clone()).unwrap_or(self.settings.output.clone());
        self.inputs = engine::list(true);
        self.outputs = engine::list(false);
        self.cable = engine::cable();
        self.cable_is_default =
            self.cable.as_ref().is_some_and(|(_, rec)| engine::default_endpoint(true).is_some_and(|d| d.id == rec.id));
        let pick = |list: &[Endpoint], id: &str| list.iter().position(|e| e.id == id);
        self.input = pick(&self.inputs, &keep_in)
            .or_else(|| engine::default_endpoint(true).and_then(|d| pick(&self.inputs, &d.id)))
            .unwrap_or(0);
        self.output = pick(&self.outputs, &keep_out)
            .or_else(|| self.cable.as_ref().and_then(|(play, _)| pick(&self.outputs, &play.id)))
            .unwrap_or(0);
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

    fn filtered(&self) -> Vec<usize> {
        (0..voices::ALL.len()).filter(|i| self.filter.is_none_or(|c| voices::ALL[*i].category == c)).collect()
    }

    fn tick(&mut self) {
        if self.shared.failed.load(Relaxed) && self.retry_at.is_none() {
            self.engine = None;
            self.healthy = false;
            self.status = self.t("Устройство пропало, переподключаюсь…", "Device lost, reconnecting…").into();
            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            self.dirty = true;
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
        let on = [self.settings.enabled, self.settings.gate, self.settings.monitor, self.settings.exclusive];
        let mut moving = false;
        for (a, on) in self.switches.iter_mut().zip(on) {
            a.target = if on { 1.0 } else { 0.0 };
            moving |= a.step(0.35);
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

    fn press(&mut self, double: bool) {
        let hit = self.hit_at(self.mouse);
        match hit {
            Hit::Tile(i) => {
                self.voice = i;
                self.settings.semitones = 0.0;
                self.apply();
                self.settings.save();
            }
            Hit::Tab(c) => {
                self.filter = c;
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
                }
                self.apply();
                if matches!(s, Switch::Monitor | Switch::Exclusive) {
                    self.restart();
                }
                self.settings.save();
            }
            Hit::Pick(p) => self.menu(p),
            Hit::MakeDefault => {
                if let Some((_, record)) = &self.cable
                    && let Err(e) = policy::set_default(&record.id)
                {
                    self.status = e.message();
                    self.healthy = false;
                }
                self.refresh_devices();
                self.dirty = true;
            }
            Hit::NameCable => {
                if let Some((play, record)) = &self.cable {
                    let exe = std::env::current_exe().unwrap_or_default();
                    let args = format!("--name-cable \"{}\" \"{}\"", play.id, record.id);
                    // A UAC prompt: the names live under HKLM like every endpoint setting.
                    let (exe, args) = (HSTRING::from(exe.as_os_str()), HSTRING::from(args));
                    unsafe {
                        ShellExecuteW(
                            self.hwnd,
                            w!("runas"),
                            PCWSTR(exe.as_ptr()),
                            PCWSTR(args.as_ptr()),
                            PCWSTR::null(),
                            0,
                        );
                    }
                    self.recheck_at = Some(Instant::now() + Duration::from_secs(4));
                }
            }
            Hit::Scrollbar(pane) => {
                let at = match pane {
                    Pane::Gallery => self.gallery_scroll.value,
                    Pane::Settings => self.settings_scroll.value,
                };
                self.drag = Some(Drag { hit, from: (self.mouse.1, at) });
                unsafe { SetCapture(self.hwnd) };
            }
            Hit::None => {}
        }
    }

    fn moved(&mut self) {
        match self.drag.as_ref().map(|d| (d.hit, d.from)) {
            Some((Hit::Slider(k), _)) => self.slider_from_x(k, self.mouse.0),
            Some((Hit::Scrollbar(pane), (y0, s0))) => {
                let (view, max) = match pane {
                    Pane::Gallery => (GALLERY.h(), self.gallery_max),
                    Pane::Settings => (PANE.h(), self.settings_max),
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
        if GALLERY.has(self.mouse) {
            let a = &mut self.gallery_scroll;
            a.target = (a.target - notches * (TILE_H + TILE_GAP)).clamp(0.0, self.gallery_max);
        } else if PANE.has(self.mouse) {
            let a = &mut self.settings_scroll;
            a.target = (a.target - notches * 72.0).clamp(0.0, self.settings_max);
        }
        self.dirty = true;
    }

    /// Arrow keys walk the gallery like a native grid view.
    fn key(&mut self, vk: u16) -> bool {
        let list = self.filtered();
        if list.is_empty() {
            return false;
        }
        let pos = list.iter().position(|i| *i == self.voice).unwrap_or(0);
        let next = match VIRTUAL_KEY(vk) {
            VK_LEFT => pos.saturating_sub(1),
            VK_RIGHT => (pos + 1).min(list.len() - 1),
            VK_UP => pos.saturating_sub(COLS),
            VK_DOWN => (pos + COLS).min(list.len() - 1),
            VK_HOME => 0,
            VK_END => list.len() - 1,
            _ => return false,
        };
        self.voice = list[next];
        self.settings.semitones = 0.0;
        self.apply();
        self.settings.save();
        // Keep the selection in view.
        let row = (next / COLS) as f32 * (TILE_H + TILE_GAP);
        let a = &mut self.gallery_scroll;
        if row < a.target {
            a.target = row;
        } else if row + TILE_H > a.target + GALLERY.h() {
            a.target = (row + TILE_H - GALLERY.h()).min(self.gallery_max);
        }
        true
    }

    /// A real Win32 popup menu under the card: native look, keyboard and accessibility.
    fn menu(&mut self, pick: Pick) {
        self.refresh_devices();
        let (items, selected) = match pick {
            Pick::Mic => (&self.inputs, self.input),
            Pick::Out => (&self.outputs, self.output),
        };
        let Some(card) = self.hits.iter().find(|(_, h)| *h == Hit::Pick(pick)).map(|(r, _)| *r) else { return };
        let scale = self.dpi / 96.0;
        let chosen = unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            for (i, e) in items.iter().enumerate() {
                let flags = if i == selected { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(e.name.as_str()));
            }
            let mut p = POINT { x: ((card.l + 52.0) * scale) as i32, y: (card.b * scale) as i32 };
            let _ = ClientToScreen(self.hwnd, &mut p);
            let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN | TPM_TOPALIGN;
            let id = TrackPopupMenuEx(menu, flags.0, p.x, p.y, self.hwnd, None);
            let _ = DestroyMenu(menu);
            id.0 as usize
        };
        if chosen > 0 {
            match pick {
                Pick::Mic => self.input = chosen - 1,
                Pick::Out => self.output = chosen - 1,
            }
            self.restart();
        }
        self.dirty = true;
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
        self.draw_scope(g)?;
        self.draw_tabs(g)?;
        self.draw_gallery(g)?;
        self.draw_settings(g)?;
        Ok(())
    }

    fn card(&self, g: &Gfx, r: R, radius: f32, hovered: bool) {
        let th = &self.theme;
        g.fill(r, radius, if hovered { th.control_hover() } else { th.card() });
        g.stroke(r, radius, th.card_stroke(), 1.0);
    }

    /// WinUI ToggleSwitch: 40x20 track, 12 px knob that slides and recolours.
    fn toggle(&self, g: &Gfx, x: f32, y: f32, which: Switch) {
        let th = &self.theme;
        let t = self.switches[which as usize].value;
        let track = R::new(x, y, 40.0, 20.0);
        if t < 1.0 {
            g.fill(track, 10.0, with_alpha(th.control_alt(), 1.0 - t));
            g.stroke(track, 10.0, with_alpha(th.strong_stroke(), 1.0 - t), 1.0);
        }
        if t > 0.0 {
            g.fill(track, 10.0, with_alpha(th.accent(), t));
        }
        let radius = if self.hover == Hit::Toggle(which) { 7.0 } else { 6.0 };
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
        let r = SCOPE;
        self.card(g, r, 8.0, false);
        let def = &voices::ALL[self.voice];
        g.text(
            def.name(self.ru),
            Font::Subtitle,
            R::new(r.l + 20.0, r.t + 12.0, 380.0, 28.0),
            Align::Start,
            th.text(),
        )?;
        let (line, color) = if !self.healthy {
            (self.status.clone(), argb(if th.dark { 0xFFFF99A4 } else { 0xFFC42B1C }))
        } else if !self.settings.enabled {
            let off = self.t("Эффект выключен: в игру идёт ваш голос", "Effect off: your own voice goes through");
            (off.into(), th.text_secondary())
        } else {
            let lat = format!("{} {} {}", self.t("задержка", "latency"), self.latency.round(), self.t("мс", "ms"));
            (format!("{} · {}", def.category.name(self.ru), lat), th.text_secondary())
        };
        g.text(&line, Font::Caption, R::new(r.l + 20.0, r.t + 40.0, 420.0, 18.0), Align::Start, color)?;

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
        let mut items: Vec<(Option<Category>, &'static str)> = vec![(None, self.t("Все", "All"))];
        items.extend(Category::ALL.iter().map(|c| (Some(*c), c.name(self.ru))));
        if self.tab_widths.len() != items.len() {
            self.tab_widths = items.iter().map(|(_, s)| g.measure(s, Font::Strong).unwrap_or(60.0)).collect();
        }
        let mut x = SCOPE.l;
        for (i, (cat, label)) in items.iter().enumerate() {
            let w = self.tab_widths[i] + 24.0;
            let r = R::new(x, TABS_Y, w, 36.0);
            let selected = self.filter == *cat;
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
            self.hits.push((r, Hit::Tab(*cat)));
            x += w;
        }
        // The SelectorBar indicator slides to the chosen category.
        let (cx, iw) = (self.tab_x.value, self.tab_w.value);
        g.fill(R::new(cx - iw / 2.0, TABS_Y + 33.0, iw, 3.0), 1.5, th.accent());
        Ok(())
    }

    fn draw_gallery(&mut self, g: &Gfx) -> Result<()> {
        let th = self.theme;
        let list = self.filtered();
        let tile_w = (GALLERY.w() - GUTTER - TILE_GAP * (COLS - 1) as f32) / COLS as f32;
        let rows = list.len().div_ceil(COLS);
        let content = rows as f32 * (TILE_H + TILE_GAP) - TILE_GAP;
        self.gallery_max = (content - GALLERY.h()).max(0.0);
        self.gallery_scroll.target = self.gallery_scroll.target.min(self.gallery_max);
        let scroll = self.gallery_scroll.value.min(self.gallery_max);

        g.clip(GALLERY);
        for (n, &i) in list.iter().enumerate() {
            let (row, col) = (n / COLS, n % COLS);
            let r = R::new(
                GALLERY.l + col as f32 * (tile_w + TILE_GAP),
                GALLERY.t + row as f32 * (TILE_H + TILE_GAP) - scroll,
                tile_w,
                TILE_H,
            );
            if !r.intersects(&GALLERY) {
                continue;
            }
            let selected = i == self.voice;
            let hovered = self.hover == Hit::Tile(i);
            g.fill(r, 4.0, if hovered { th.control_hover() } else { th.card() });
            if selected {
                g.stroke(r, 4.0, th.accent(), 2.0);
            } else {
                g.stroke(r, 4.0, th.card_stroke(), 1.0);
            }
            // The icon is the voice's own waveform.
            let wave = &self.glyphs[i];
            let (gl, gw, gy) = (r.l + 14.0, r.w() - 28.0, r.t + 24.0);
            let last = (wave.len() - 1) as f32;
            let pts: Vec<(f32, f32)> =
                wave.iter().enumerate().map(|(k, s)| (gl + gw * k as f32 / last, gy - s * 11.0)).collect();
            g.line(&pts, if selected { th.accent() } else { th.text_secondary() }, 1.4)?;
            let name = R::new(r.l + 14.0, r.t + 44.0, r.w() - 28.0, 22.0);
            g.text(voices::ALL[i].name(self.ru), Font::Body, name, Align::Start, th.text())?;
            let visible = R { l: r.l, t: r.t.max(GALLERY.t), r: r.r, b: r.b.min(GALLERY.b) };
            self.hits.push((visible, Hit::Tile(i)));
        }
        g.unclip();
        self.scrollbar(g, GALLERY, scroll, self.gallery_max, Pane::Gallery);
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
        let scroll = self.settings_scroll.value.min(self.settings_max);
        let (x, w) = (PANE.l, PANE.w() - GUTTER);
        let mut y = PANE.t - scroll;
        g.clip(PANE);

        let section = |g: &Gfx, y: &mut f32, title: &str| -> Result<()> {
            g.text(title, Font::Strong, R::new(x + 2.0, *y, w, 20.0), Align::Start, th.text())?;
            *y += 28.0;
            Ok(())
        };

        section(g, &mut y, self.t("Голос", "Voice"))?;
        self.slider_card(g, &mut y, Knob::Tone, glyph::TONE, self.t("Тон", "Pitch"))?;
        self.slider_card(g, &mut y, Knob::Volume, glyph::VOLUME, self.t("Громкость", "Volume"))?;
        self.slider_card(g, &mut y, Knob::Mix, glyph::MIX, self.t("Свой голос в миксе", "Original voice in mix"))?;
        y += 20.0;

        section(g, &mut y, self.t("Микрофон", "Microphone"))?;
        let mic = self.inputs.get(self.input).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Mic), glyph::MIC, self.t("Устройство", "Device"), &mic)?;
        let fast = self.t("Низкая задержка", "Low latency");
        let mode = match (self.settings.exclusive, self.shared.mic_exclusive.load(Relaxed)) {
            (true, true) => {
                self.t("Эксклюзивный режим: микрофон только у Goby", "Exclusive mode: only Goby uses the mic")
            }
            (true, false) => self.t("Микрофон не дал эксклюзивный режим", "The mic refused exclusive mode"),
            _ => self.t("Общий режим, пакеты по 10 мс", "Shared mode, 10 ms packets"),
        };
        self.row_card(g, &mut y, Hit::Toggle(Switch::Exclusive), glyph::SPEED, fast, mode)?;
        self.slider_card(g, &mut y, Knob::Gain, glyph::VOLUME, self.t("Усиление", "Gain"))?;
        let gate = self.t("Глушит клавиатуру и шум между фразами", "Mutes keys and noise between phrases");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Gate), glyph::GATE, self.t("Шумовой гейт", "Noise gate"), gate)?;
        self.slider_card(g, &mut y, Knob::Gate, glyph::GATE, self.t("Порог гейта", "Gate threshold"))?;
        y += 20.0;

        section(g, &mut y, self.t("Вывод", "Output"))?;
        let out = self.outputs.get(self.output).map_or("—", |e| short(&e.name)).to_string();
        self.row_card(g, &mut y, Hit::Pick(Pick::Out), glyph::SPEAKERS, self.t("Куда отдавать", "Send to"), &out)?;
        let mon = self.t("В наушники по умолчанию", "To your default headphones");
        let hear = self.t("Слышать себя", "Hear myself");
        self.row_card(g, &mut y, Hit::Toggle(Switch::Monitor), glyph::HEADPHONE, hear, mon)?;
        if let Some((play, record)) = self.cable.clone() {
            let games = self.t("Микрофон для игр", "Mic for games");
            if self.cable_is_default {
                let done = self.t("Готово: CS2 и чаты слышат Goby", "Done: CS2 and chats hear Goby");
                self.row_card(g, &mut y, Hit::None, glyph::GAME, games, done)?;
            } else {
                let how = format!("{} «{}»", self.t("Сделать по умолчанию:", "Make default:"), short(&record.name));
                self.row_card(g, &mut y, Hit::MakeDefault, glyph::GAME, games, &how)?;
            }
            if !play.name.starts_with("Goby") {
                let name = self.t("Назвать кабель «Goby»", "Name the cable Goby");
                let why = self.t("Чтобы не путаться в устройствах", "So it is easy to spot in device lists");
                self.row_card(g, &mut y, Hit::NameCable, glyph::RENAME, name, why)?;
            }
        } else {
            let none =
                self.t("Установите VB-Cable, чтобы голос шёл в игры", "Install VB-Cable to route the voice into games");
            self.row_card(g, &mut y, Hit::None, glyph::GAME, self.t("Виртуальный кабель", "Virtual cable"), none)?;
        }

        g.unclip();
        let content = y + scroll - PANE.t;
        self.settings_max = (content - PANE.h()).max(0.0);
        self.scrollbar(g, PANE, scroll, self.settings_max, Pane::Settings);
        Ok(())
    }

    /// SettingsCard with an icon, a title and a value, and a slider underneath.
    fn slider_card(&mut self, g: &Gfx, y: &mut f32, k: Knob, icon: u16, title: &str) -> Result<()> {
        let th = self.theme;
        let r = R::new(PANE.l, *y, PANE.w() - GUTTER, 84.0);
        if r.intersects(&PANE) {
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
        let r = R::new(PANE.l, *y, PANE.w() - GUTTER, 68.0);
        if r.intersects(&PANE) {
            self.card(g, r, 4.0, self.hover == hit);
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
                Hit::Pick(_) => g.icon(glyph::CHEVRON_DOWN, Font::Icon12, side, th.text_secondary())?,
                Hit::None if self.cable_is_default => g.icon(glyph::CHECK, Font::Icon12, side, th.accent())?,
                Hit::None => {}
                _ => g.icon(glyph::OPEN, Font::Icon12, side, th.text_secondary())?,
            }
            let visible = R { l: r.l, t: r.t.max(PANE.t), r: r.r, b: r.b.min(PANE.b) };
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
                let all = Margins { left: -1, right: -1, top: -1, bottom: -1 };
                self.mica = DwmExtendFrameIntoClientArea(self.hwnd, &all).is_ok();
            }
            for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
                if let Some(icon) = make_icon(size, self.theme.accent_rgb()) {
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

/// The app icon: the Minion voice's own waveform in the system accent colour.
fn make_icon(size: i32, rgb: u32) -> Option<HICON> {
    let n = size as usize;
    let wave = voices::glyph(&voices::ALL[1], 64);
    let pts: Vec<(f32, f32)> = wave
        .iter()
        .enumerate()
        .map(|(i, s)| (2.0 + (size as f32 - 4.0) * i as f32 / 63.0, size as f32 / 2.0 - s * size as f32 * 0.32))
        .collect();
    let half = (size as f32 / 16.0).max(1.0);
    let mut px = vec![0u32; n * n];
    for (i, p) in px.iter_mut().enumerate() {
        let (x, y) = ((i % n) as f32 + 0.5, (i / n) as f32 + 0.5);
        let d = pts
            .windows(2)
            .map(|s| {
                let ((ax, ay), (bx, by)) = (s[0], s[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
            })
            .fold(f32::MAX, f32::min);
        let a8 = ((half + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
        // Premultiplied BGRA.
        let c = |v: u32| v * a8 / 255;
        *p = (a8 << 24) | (c((rgb >> 16) & 0xff) << 16) | (c((rgb >> 8) & 0xff) << 8) | c(rgb & 0xff);
    }
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
                if app.dirty && !IsIconic(hwnd).as_bool() && !app.covered() {
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
                app.press(msg == WM_LBUTTONDBLCLK);
                app.hover = app.hit_at(app.mouse);
                app.dirty = true;
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
            WM_SIZE => {
                let (w, h) = ((lp.0 & 0xffff) as u32, ((lp.0 >> 16) & 0xffff) as u32);
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
            WM_DESTROY => {
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

fn window_size(dpi: u32, style: WINDOW_STYLE) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: (WIDTH * dpi as f32 / 96.0).round() as i32,
        bottom: (HEIGHT * dpi as f32 / 96.0).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
    }
    (r.right - r.left, r.bottom - r.top)
}

pub fn run() -> Result<()> {
    unsafe {
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
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let (w, h) = window_size(GetDpiForSystem(), style);
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

        // The monitor the window landed on may not be the primary one.
        let dpi = GetDpiForWindow(hwnd);
        if dpi != GetDpiForSystem() {
            let (w, h) = window_size(dpi, style);
            let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER);
        }

        let mut app = Box::new(App::new(hwnd));
        app.apply_frame();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        SetTimer(Some(hwnd), 1, 33, None);
        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
