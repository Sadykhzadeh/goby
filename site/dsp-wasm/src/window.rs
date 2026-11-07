//! The window on the website: the app's own `ui` on the sample `demo` host, drawn by a
//! Canvas 2D context in docs/window.js. This file only passes calls across: drawing goes
//! out as imports, pointer, keys and frames come in as exports. Coordinates are DIPs.

use std::time::Duration;

use crate::demo::{self, DemoHost};
use crate::draw::{Align, Canvas, Color, Font, R, glyph};
use crate::host::{Hotkey, Key, Link, SCOPE_LEN};
use crate::theme::Theme;
use crate::ui::{App, Popup, Switch};

unsafe extern "C" {
    fn begin();
    fn fill(l: f32, t: f32, r: f32, b: f32, radius: f32, rgba: u32);
    fn stroke(l: f32, t: f32, r: f32, b: f32, radius: f32, rgba: u32, width: f32);
    fn circle(x: f32, y: f32, radius: f32, rgba: u32);
    fn line(xy: *const f32, n: usize, rgba: u32, width: f32);
    fn text(s: *const u8, len: usize, font: u32, l: f32, t: f32, r: f32, b: f32, align: u32, rgba: u32);
    fn shape(d: *const u8, len: usize, filled: bool, l: f32, t: f32, size: f32, rgba: u32);
    fn measure(s: *const u8, len: usize, font: u32) -> f32;
    fn clip(l: f32, t: f32, r: f32, b: f32);
    fn unclip();
    fn shift(dy: f32, opacity: f32);
    fn unshift();
    /// Opens a menu: labels separated by newlines, the checked one (or -1), and where.
    fn menu(labels: *const u8, len: usize, checked: i32, x: f32, y: f32);
}

fn rgba(c: Color) -> u32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    b(c.r) << 24 | b(c.g) << 16 | b(c.b) << 8 | b(c.a)
}

struct Web;

impl Canvas for Web {
    fn dpi(&self) -> f32 {
        96.0
    }
    fn fill(&self, r: R, radius: f32, c: Color) {
        unsafe { fill(r.l, r.t, r.r, r.b, radius, rgba(c)) }
    }
    fn stroke(&self, r: R, radius: f32, c: Color, width: f32) {
        unsafe { stroke(r.l, r.t, r.r, r.b, radius, rgba(c), width) }
    }
    fn circle(&self, x: f32, y: f32, radius: f32, c: Color) {
        unsafe { circle(x, y, radius, rgba(c)) }
    }
    fn line(&self, pts: &[(f32, f32)], c: Color, width: f32) {
        let xy: Vec<f32> = pts.iter().flat_map(|&(x, y)| [x, y]).collect();
        unsafe { line(xy.as_ptr(), pts.len(), rgba(c), width) }
    }
    fn text(&self, s: &str, f: Font, r: R, align: Align, c: Color) {
        unsafe { text(s.as_ptr(), s.len(), f as u32, r.l, r.t, r.r, r.b, align as u32, rgba(c)) }
    }
    fn icon(&self, code: u16, f: Font, r: R, c: Color) {
        let size = if f == Font::Icon12 { 12.0 } else { 16.0 };
        let (l, t) = (r.l + (r.w() - size) / 2.0, r.t + (r.h() - size) / 2.0);
        for (filled, d) in glyph::shape(code) {
            unsafe { shape(d.as_ptr(), d.len(), *filled, l, t, size, rgba(c)) }
        }
    }
    fn measure(&self, s: &str, f: Font) -> f32 {
        unsafe { measure(s.as_ptr(), s.len(), f as u32) }
    }
    fn picture(&self, _: &str, _: u32, _: &mut dyn FnMut() -> Option<Vec<u8>>, _: R) -> bool {
        false
    }
    fn clip(&self, r: R) {
        unsafe { clip(r.l, r.t, r.r, r.b) }
    }
    fn unclip(&self) {
        unsafe { unclip() }
    }
    fn shift(&self, dy: f32, opacity: f32) {
        unsafe { shift(dy, opacity) }
    }
    fn unshift(&self) {
        unsafe { unshift() }
    }
}

struct State {
    app: App<DemoHost>,
    popup: Option<Popup>,
    last_tick: f64,
}

static mut STATE: Option<State> = None;

#[allow(static_mut_refs)]
fn state() -> &'static mut State {
    unsafe { STATE.as_mut().expect("ui_start first") }
}

fn app() -> &'static mut App<DemoHost> {
    &mut state().app
}

/// Where the page writes the browser's language, `navigator.language`, before `ui_start`.
static mut LANGUAGE: [u8; 35] = [0; 35];

#[unsafe(no_mangle)]
pub extern "C" fn ui_language() -> *mut u8 {
    (&raw mut LANGUAGE).cast()
}

/// Starts the window at `w`×`h` DIPs. `still` asks for no motion, and the first `language`
/// bytes of `ui_language` name the browser's language.
#[unsafe(no_mangle)]
pub extern "C" fn ui_start(w: f32, h: f32, still: bool, language: usize) {
    // Windows' gold accent, the amber of the site, on the dark theme.
    let theme = Theme::new(true, [0xFFE38C, 0xFFB900, 0xF7A100, 0xD18A00, 0x9A6400, 0x6E4500, 0x412700]);
    let mut host = DemoHost::default();
    host.animations = !still;
    let tag = unsafe { LANGUAGE };
    host.language = std::str::from_utf8(&tag[..language.min(tag.len())]).unwrap_or("").to_string();
    let mut app = App::new(host, demo::settings(), theme, 96.0);
    app.still = still;
    app.shown = true;
    app.resize(w, h);
    unsafe { STATE = Some(State { app, popup: None, last_tick: f64::MIN }) };
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_resize(w: f32, h: f32) {
    app().resize(w, h);
}

/// One animation frame at `now` ms; `heard` says the microphone samples are new. Ticks at
/// the app's 30 per second and draws when something changed. True while frames are wanted.
#[unsafe(no_mangle)]
pub extern "C" fn ui_frame(now: f64, heard: bool) -> bool {
    let s = state();
    s.app.host.clock = Duration::from_secs_f64(now.max(0.0) / 1000.0);
    s.app.host.fresh |= heard;
    let due = now - s.last_tick >= 32.0;
    if due {
        s.last_tick = now;
        s.app.tick();
    }
    let drew = s.app.dirty;
    if drew {
        unsafe { begin() };
        s.app.draw(&Web);
        s.app.dirty = false;
    }
    drew || !due
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_move(x: f32, y: f32) {
    app().moved((x, y));
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_leave() {
    app().left();
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_down(x: f32, y: f32, double: bool) {
    let s = state();
    if let Some(p) = s.app.down((x, y), double) {
        let labels = p.labels.join("\n");
        unsafe { menu(labels.as_ptr(), labels.len(), p.checked.map_or(-1, |c| c as i32), p.at.0, p.at.1) };
        s.popup = Some(p);
    }
}

/// The open menu closed on item `i`, or on nothing when `i` is negative.
#[unsafe(no_mangle)]
pub extern "C" fn ui_menu(i: i32) {
    let s = state();
    let Some(p) = s.popup.take() else { return };
    let key = usize::try_from(i).ok().and_then(|i| p.keys.get(i).cloned());
    s.app.menu_done(p.pick, key);
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_up() {
    app().up();
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_right(x: f32, y: f32) {
    app().right_click((x, y));
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_wheel(x: f32, y: f32, notches: f32) {
    app().wheel((x, y), notches);
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_drag_scroll(x: f32, y: f32, dy: f32) -> bool {
    app().drag_scroll((x, y), dy)
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_draggable(x: f32, y: f32) -> bool {
    app().draggable_at((x, y))
}

/// Arrows, Home, End, Escape and Backspace, numbered in that order.
#[unsafe(no_mangle)]
pub extern "C" fn ui_key(k: u32) -> bool {
    let keys = [Key::Left, Key::Right, Key::Up, Key::Down, Key::Home, Key::End, Key::Escape, Key::Back];
    keys.get(k as usize).is_some_and(|k| app().key(*k))
}

/// Ctrl+Alt+G (0), Page Down (1), Page Up (2), when the hotkeys switch is on.
#[unsafe(no_mangle)]
pub extern "C" fn ui_hotkey(k: u32) -> bool {
    let a = app();
    let action = [Hotkey::Toggle, Hotkey::Next, Hotkey::Previous].get(k as usize).copied();
    let on = a.host.hotkeys;
    if let Some(action) = action.filter(|_| on) {
        a.hotkey(action);
    }
    on
}

/// Hear yourself on or off, from the page's own button.
#[unsafe(no_mangle)]
pub extern "C" fn ui_monitor(on: bool) {
    let a = app();
    if a.is_on(Switch::Monitor) != on {
        a.flip(Switch::Monitor);
    }
}

/// What the page has to act on after an event: the voice should be heard (bit 0), and a
/// link to open (bits 8 and up: 1 author, 2 VB-Cable).
#[unsafe(no_mangle)]
pub extern "C" fn ui_wants() -> u32 {
    let h = &mut app().host;
    let link = match h.opened.take() {
        Some(Link::Author) => 1,
        Some(Link::Cable) => 2,
        _ => 0,
    };
    h.live as u32 | link << 8
}

/// The voice and microphone settings for the audio worklet: voice, pitch, volume, gain,
/// gate (NaN for off), mix and the effect switch, as seven floats.
#[unsafe(no_mangle)]
pub extern "C" fn ui_params() -> *const f32 {
    static mut OUT: [f32; 7] = [0.0; 7];
    let p = app().host.params.get();
    unsafe {
        OUT = [p.voice as f32, p.tone, p.volume, p.gain_db, p.gate_db, p.dry_mix, p.enabled as u8 as f32];
        (&raw const OUT).cast()
    }
}

/// Where the page writes the microphone before and after the voice, `SCOPE_LEN` each.
#[unsafe(no_mangle)]
pub extern "C" fn ui_heard() -> *mut f32 {
    app().host.heard.as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_scope_len() -> usize {
    SCOPE_LEN
}

#[unsafe(no_mangle)]
pub extern "C" fn ui_latency(ms: f32) {
    app().host.latency = ms;
}
