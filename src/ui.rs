//! The window: plain Win32 + Direct2D + DirectWrite, drawn by hand in DIPs.
//!
//! One fixed-size layout, retained only as a handful of fields on `App`;
//! everything is repainted at ~30 fps because the scope is live anyway.

use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Interface, Result, w};
use windows_numerics::Vector2;

use crate::dsp::Preset;
use crate::engine::{self, Endpoint, Engine, SCOPE_LEN, Shared};
use crate::settings::Settings;

static BASTELEUR: &[u8] = include_bytes!("../ui/fonts/Basteleur-Bold.ttf");

// Warm graphite, not slate. The only colour is a pale, tonal lemon.
const BASE: u32 = 0x141311;
const SURFACE: u32 = 0x1b1916;
const FIELD: u32 = 0x211f1b;
const EDGE: u32 = 0x2c2924;
const INK: u32 = 0xece8df;
const MUTED: u32 = 0xa8a296;
const FAINT: u32 = 0x615c54;
const WATERMARK: u32 = 0x262420;
const ACCENT: u32 = 0xe8dc96;
const WARN: u32 = 0xe9a07a;

// Layout, in DIPs.
const WIDTH: f32 = 580.0;
const HEIGHT: f32 = 760.0;
const LEFT: f32 = 28.0;
const RIGHT: f32 = WIDTH - 28.0;
const SCOPE: R = R { l: LEFT, t: 76.0, r: RIGHT, b: 286.0 };
const PRESETS: R = R { l: LEFT, t: 304.0, r: RIGHT, b: 368.0 };
const TONE_Y: f32 = 392.0;
const VOLUME_Y: f32 = 474.0;
const RULER_H: f32 = 62.0;
const COMBO_X: f32 = 168.0;
const COMBO_H: f32 = 34.0;
const MIC_Y: f32 = 564.0;
const OUT_Y: f32 = 608.0;
const MONITOR_Y: f32 = 658.0;
const FOOT_Y: f32 = 708.0;
const ITEM_H: f32 = 32.0;
const LINK_TEXT: &str = "Настройки звука";
// Lives in Win32_UI_Controls; not worth pulling that whole feature in for one number.
const WM_MOUSELEAVE: u32 = 0x02A3;
const LIST_MAX: usize = 7;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct R {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

impl R {
    const fn new(l: f32, t: f32, w: f32, h: f32) -> R {
        R { l, t, r: l + w, b: t + h }
    }
    fn w(&self) -> f32 {
        self.r - self.l
    }
    fn h(&self) -> f32 {
        self.b - self.t
    }
    fn has(&self, (x, y): (f32, f32)) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.l, top: self.t, right: self.r, bottom: self.b }
    }
}

fn rgb(hex: u32) -> D2D1_COLOR_F {
    let c = |s: u32| ((hex >> s) & 0xff) as f32 / 255.0;
    D2D1_COLOR_F { r: c(16), g: c(8), b: c(0), a: 1.0 }
}

fn mix(a: u32, b: u32, t: f32) -> D2D1_COLOR_F {
    let (a, b) = (rgb(a), rgb(b));
    let l = |x: f32, y: f32| x + (y - x) * t.clamp(0.0, 1.0);
    D2D1_COLOR_F { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: 1.0 }
}

fn v(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Which {
    Mic,
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    None,
    Preset(usize),
    Tone,
    Volume,
    Combo(Which),
    Item(Which, usize),
    Monitor,
    Link,
}

struct Fonts {
    body: IDWriteTextFormat,
    strong: IDWriteTextFormat,
    small: IDWriteTextFormat,
    readout: IDWriteTextFormat,
    field: IDWriteTextFormat,
    mark: IDWriteTextFormat,
    preset: IDWriteTextFormat,
    watermark: IDWriteTextFormat,
}

struct Gfx {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}

struct App {
    hwnd: HWND,
    d2d: ID2D1Factory,
    dw: IDWriteFactory,
    fonts: Fonts,
    round: ID2D1StrokeStyle,
    gfx: Option<Gfx>,
    dpi: f32,

    settings: Settings,
    inputs: Vec<Endpoint>,
    outputs: Vec<Endpoint>,
    input: usize,
    output: usize,
    preset: Preset,

    shared: Arc<Shared>,
    engine: Option<Engine>,
    status: String,
    healthy: bool,
    retry_at: Option<Instant>,

    mouse: (f32, f32),
    hover: Hit,
    drag: Option<Hit>,
    open: Option<Which>,
    scroll: usize,
    /// Width of the "sound settings" link text, measured once; its hit area hangs off it.
    link_w: f32,
    glow: [f32; 6],
    knob: f32,
    latency: f32,
    glyphs: Vec<Vec<(f32, f32)>>,
    raw: Box<[f32; SCOPE_LEN]>,
    voiced: Box<[f32; SCOPE_LEN]>,
}

fn name_of(p: Preset) -> &'static str {
    match p {
        Preset::Off => "Off",
        Preset::Minion => "Minion",
        Preset::Helium => "Helium",
        Preset::Giant => "Giant",
        Preset::Robot => "Robot",
        Preset::Radio => "Radio",
    }
}

/// Each preset is drawn as the waveform it makes: one invented icon family.
fn glyph(p: Preset) -> Vec<(f32, f32)> {
    let wave = |cycles: f32, amp: f32, clip: f32| -> Vec<(f32, f32)> {
        (0..=96)
            .map(|i| {
                let t = i as f32 / 96.0;
                let s = ((t * cycles * std::f32::consts::TAU).sin() * amp).clamp(-clip, clip);
                (t * 44.0, 9.0 - s * 7.0)
            })
            .collect()
    };
    match p {
        Preset::Off => vec![(0.0, 9.0), (44.0, 9.0)],
        Preset::Minion => wave(5.0, 0.75, 1.0),
        Preset::Helium => wave(9.0, 0.55, 1.0),
        Preset::Giant => wave(1.25, 1.0, 1.0),
        Preset::Robot => {
            let mut pts = vec![(0.0, 13.0)];
            for (i, x) in [7.0, 15.0, 22.0, 30.0, 37.0].iter().enumerate() {
                let (from, to) = if i % 2 == 0 { (13.0, 5.0) } else { (5.0, 13.0) };
                pts.push((*x, from));
                pts.push((*x, to));
            }
            pts.push((44.0, 5.0));
            pts
        }
        Preset::Radio => wave(3.0, 1.3, 0.45),
    }
}

fn short(name: &str) -> &str {
    name.split(" (").next().unwrap_or(name)
}

impl App {
    fn new(hwnd: HWND) -> Result<App> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let fonts = make_fonts(&dw)?;
            let round = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 1.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;

            let settings = Settings::load();
            let mut app = App {
                hwnd,
                d2d,
                dw,
                fonts,
                round,
                gfx: None,
                dpi: GetDpiForWindow(hwnd) as f32,
                preset: Preset::from_index(settings.preset),
                settings,
                inputs: Vec::new(),
                outputs: Vec::new(),
                input: 0,
                output: 0,
                shared: Arc::new(Shared::default()),
                engine: None,
                status: String::new(),
                healthy: true,
                retry_at: None,
                mouse: (-1.0, -1.0),
                hover: Hit::None,
                drag: None,
                open: None,
                scroll: 0,
                link_w: 0.0,
                glow: [0.0; 6],
                knob: 0.0,
                latency: 0.0,
                glyphs: Preset::ALL.iter().map(|p| glyph(*p)).collect(),
                raw: Box::new([0.0; SCOPE_LEN]),
                voiced: Box::new([0.0; SCOPE_LEN]),
            };
            app.knob = if app.settings.monitor { 1.0 } else { 0.0 };
            app.link_w = app.measure(LINK_TEXT, &app.fonts.strong)?;
            app.refresh_devices();
            app.apply_voice();
            app.restart();
            Ok(app)
        }
    }

    fn refresh_devices(&mut self) {
        self.inputs = engine::list(true);
        self.outputs = engine::list(false);
        let pick = |list: &[Endpoint], id: &str| list.iter().position(|e| e.id == id);
        self.input = pick(&self.inputs, &self.settings.input)
            .or_else(|| engine::default_endpoint(true).and_then(|d| pick(&self.inputs, &d.id)))
            .unwrap_or(0);
        self.output = pick(&self.outputs, &self.settings.output)
            .or_else(|| self.outputs.iter().position(|e| e.name.to_ascii_lowercase().contains("cable input")))
            .unwrap_or(0);
    }

    fn apply_voice(&mut self) {
        self.settings.preset = self.preset.index();
        self.shared.set(self.preset, self.settings.semitones, self.settings.volume);
    }

    fn restart(&mut self) {
        // Release the devices before opening them again.
        self.engine = None;
        self.retry_at = None;
        self.shared.failed.store(false, Relaxed);
        let (Some(mic), Some(out)) = (self.inputs.get(self.input), self.outputs.get(self.output)) else {
            self.healthy = false;
            self.status = if self.inputs.is_empty() {
                "Микрофон не найден".into()
            } else {
                "Нет выходов звука".into()
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
        match Engine::start(&mic.id, &targets, self.shared.clone()) {
            Ok(e) => {
                self.healthy = true;
                self.status = format!("Голос идёт в {}", short(&out.name));
                self.engine = Some(e);
            }
            Err(e) => {
                self.healthy = false;
                self.status = e;
                self.retry_at = Some(Instant::now() + Duration::from_secs(2));
            }
        }
        self.settings.save();
    }

    fn tick(&mut self) {
        if self.shared.failed.load(Relaxed) && self.retry_at.is_none() {
            self.engine = None;
            self.healthy = false;
            self.status = "Устройство пропало, переподключаюсь…".into();
            self.retry_at = Some(Instant::now() + Duration::from_secs(2));
        }
        if self.retry_at.is_some_and(|t| Instant::now() >= t) {
            self.refresh_devices();
            self.restart();
        }
        for (i, g) in self.glow.iter_mut().enumerate() {
            let target = if i == self.preset.index() {
                1.0
            } else if self.hover == Hit::Preset(i) {
                0.45
            } else {
                0.0
            };
            *g += (target - *g) * 0.35;
        }
        let k = if self.settings.monitor { 1.0 } else { 0.0 };
        self.knob += (k - self.knob) * 0.4;
        // The live figure jitters by a packet; ease it so the readout does not flicker.
        let measured = self.shared.latency_ms();
        self.latency = if self.latency == 0.0 { measured } else { self.latency + (measured - self.latency) * 0.05 };
    }

    // ---------- input ----------

    fn list_rect(&self, which: Which) -> (R, usize) {
        let (y, count) = match which {
            Which::Mic => (MIC_Y, self.inputs.len()),
            Which::Out => (OUT_Y, self.outputs.len()),
        };
        let shown = count.min(LIST_MAX);
        let h = shown as f32 * ITEM_H + 8.0;
        let below = y + COMBO_H + 4.0;
        let top = if below + h <= HEIGHT - 8.0 { below } else { y - 4.0 - h };
        (R { l: COMBO_X, t: top, r: RIGHT, b: top + h }, shown)
    }

    fn hit(&self, p: (f32, f32)) -> Hit {
        if let Some(which) = self.open {
            let (r, shown) = self.list_rect(which);
            if r.has(p) {
                let i = ((p.1 - r.t - 4.0) / ITEM_H).floor().max(0.0) as usize;
                return Hit::Item(which, (i.min(shown.saturating_sub(1))) + self.scroll);
            }
            return Hit::None;
        }
        if PRESETS.has(p) {
            let i = ((p.0 - PRESETS.l) / (PRESETS.w() / 6.0)) as usize;
            return Hit::Preset(i.min(5));
        }
        if R::new(LEFT - 6.0, TONE_Y + 20.0, RIGHT - LEFT + 12.0, RULER_H - 14.0).has(p) {
            return Hit::Tone;
        }
        if R::new(LEFT - 6.0, VOLUME_Y + 20.0, RIGHT - LEFT + 12.0, RULER_H - 14.0).has(p) {
            return Hit::Volume;
        }
        if R::new(COMBO_X, MIC_Y, RIGHT - COMBO_X, COMBO_H).has(p) {
            return Hit::Combo(Which::Mic);
        }
        if R::new(COMBO_X, OUT_Y, RIGHT - COMBO_X, COMBO_H).has(p) {
            return Hit::Combo(Which::Out);
        }
        if R::new(COMBO_X, MONITOR_Y, 260.0, 24.0).has(p) {
            return Hit::Monitor;
        }
        if (R { l: RIGHT - 18.0 - self.link_w, t: FOOT_Y + 4.0, r: RIGHT + 4.0, b: FOOT_Y + 32.0 }).has(p) {
            return Hit::Link;
        }
        Hit::None
    }

    fn ruler_value(&mut self, which: Hit, x: f32) {
        let t = ((x - LEFT) / (RIGHT - LEFT)).clamp(0.0, 1.0);
        match which {
            Hit::Tone => self.settings.semitones = (t * 24.0 - 12.0).round(),
            Hit::Volume => self.settings.volume = (t * 2.0 / 0.05).round() * 0.05,
            _ => return,
        }
        self.apply_voice();
    }

    fn press(&mut self, double: bool) {
        let hit = self.hit(self.mouse);
        if let Some(which) = self.open.take() {
            if let Hit::Item(_, i) = hit {
                match which {
                    Which::Mic => self.input = i,
                    Which::Out => self.output = i,
                }
                self.restart();
            }
            return;
        }
        match hit {
            Hit::Preset(i) => {
                self.preset = Preset::ALL[i];
                self.settings.semitones = self.preset.semitones();
                self.apply_voice();
                self.settings.save();
            }
            Hit::Tone | Hit::Volume if double => {
                if hit == Hit::Tone {
                    self.settings.semitones = 0.0;
                } else {
                    self.settings.volume = 1.0;
                }
                self.apply_voice();
                self.settings.save();
            }
            Hit::Tone | Hit::Volume => {
                self.drag = Some(hit);
                unsafe { SetCapture(self.hwnd) };
                self.ruler_value(hit, self.mouse.0);
            }
            Hit::Combo(which) => {
                // Devices come and go; re-read them every time a list opens.
                self.refresh_devices_keep();
                self.open = Some(which);
                let (sel, count) = match which {
                    Which::Mic => (self.input, self.inputs.len()),
                    Which::Out => (self.output, self.outputs.len()),
                };
                self.scroll = sel.saturating_sub(LIST_MAX - 1).min(count.saturating_sub(LIST_MAX));
            }
            Hit::Monitor => {
                self.settings.monitor = !self.settings.monitor;
                self.restart();
            }
            Hit::Link => {
                let _ = std::process::Command::new("control.exe").arg("mmsys.cpl,,1").spawn();
            }
            Hit::None | Hit::Item(..) => {}
        }
    }

    fn refresh_devices_keep(&mut self) {
        let (mic, out) =
            (self.inputs.get(self.input).map(|e| e.id.clone()), self.outputs.get(self.output).map(|e| e.id.clone()));
        self.inputs = engine::list(true);
        self.outputs = engine::list(false);
        self.input = mic.and_then(|id| self.inputs.iter().position(|e| e.id == id)).unwrap_or(0);
        self.output = out.and_then(|id| self.outputs.iter().position(|e| e.id == id)).unwrap_or(0);
    }

    fn release(&mut self) {
        if self.drag.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
            self.settings.save();
        }
    }

    fn wheel(&mut self, up: bool) {
        if let Some(which) = self.open {
            let count = match which {
                Which::Mic => self.inputs.len(),
                Which::Out => self.outputs.len(),
            };
            let max = count.saturating_sub(LIST_MAX);
            self.scroll = if up { self.scroll.saturating_sub(1) } else { (self.scroll + 1).min(max) };
            return;
        }
        match self.hit(self.mouse) {
            Hit::Tone => {
                self.settings.semitones = (self.settings.semitones + if up { 1.0 } else { -1.0 }).clamp(-12.0, 12.0);
            }
            Hit::Volume => {
                let v = self.settings.volume + if up { 0.05 } else { -0.05 };
                self.settings.volume = ((v / 0.05).round() * 0.05).clamp(0.0, 2.0);
            }
            _ => return,
        }
        self.apply_voice();
        self.settings.save();
    }

    // ---------- drawing ----------

    fn ensure_gfx(&mut self) -> Result<()> {
        if self.gfx.is_some() {
            return Ok(());
        }
        unsafe {
            let mut rc = RECT::default();
            GetClientRect(self.hwnd, &mut rc)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_UNKNOWN, alphaMode: D2D1_ALPHA_MODE_UNKNOWN },
                dpiX: self.dpi,
                dpiY: self.dpi,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let rt = self.d2d.CreateHwndRenderTarget(&props, &hwnd_props)?;
            let brush = rt.CreateSolidColorBrush(&rgb(INK), None)?;
            self.gfx = Some(Gfx { rt, brush });
        }
        Ok(())
    }

    fn paint(&mut self) -> Result<()> {
        self.ensure_gfx()?;
        let g = self.gfx.as_ref().unwrap();
        let result = unsafe {
            g.rt.BeginDraw();
            g.rt.Clear(Some(&rgb(BASE)));
            self.draw(g)?;
            g.rt.EndDraw(None, None)
        };
        if let Err(e) = result {
            if e.code() == D2DERR_RECREATE_TARGET {
                self.gfx = None;
            } else {
                return Err(e);
            }
        }
        Ok(())
    }

    fn fill(&self, g: &Gfx, r: R, radius: f32, c: D2D1_COLOR_F) {
        unsafe {
            g.brush.SetColor(&c);
            g.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }, &g.brush);
        }
    }

    fn outline(&self, g: &Gfx, r: R, radius: f32, c: D2D1_COLOR_F) {
        unsafe {
            g.brush.SetColor(&c);
            let half = R { l: r.l + 0.5, t: r.t + 0.5, r: r.r - 0.5, b: r.b - 0.5 };
            g.rt.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: half.d2d(), radiusX: radius, radiusY: radius },
                &g.brush,
                1.0,
                None,
            );
        }
    }

    fn line(&self, g: &Gfx, pts: &[(f32, f32)], c: D2D1_COLOR_F, width: f32) -> Result<()> {
        if pts.len() < 2 {
            return Ok(());
        }
        unsafe {
            let geo = self.d2d.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.BeginFigure(v(pts[0].0, pts[0].1), D2D1_FIGURE_BEGIN_HOLLOW);
            for p in &pts[1..] {
                sink.AddLine(v(p.0, p.1));
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            g.brush.SetColor(&c);
            g.rt.DrawGeometry(&geo, &g.brush, width, &self.round);
        }
        Ok(())
    }

    fn measure(&self, s: &str, f: &IDWriteTextFormat) -> Result<f32> {
        unsafe {
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.dw.CreateTextLayout(&wide, f, 4096.0, 256.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok(m.width)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        g: &Gfx,
        s: &str,
        f: &IDWriteTextFormat,
        r: R,
        h: DWRITE_TEXT_ALIGNMENT,
        vert: DWRITE_PARAGRAPH_ALIGNMENT,
        c: D2D1_COLOR_F,
    ) -> Result<()> {
        unsafe {
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.dw.CreateTextLayout(&wide, f, r.w().max(1.0), r.h().max(1.0))?;
            layout.SetTextAlignment(h)?;
            layout.SetParagraphAlignment(vert)?;
            g.brush.SetColor(&c);
            g.rt.DrawTextLayout(v(r.l, r.t), &layout, &g.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        Ok(())
    }

    fn draw(&self, g: &Gfx) -> Result<()> {
        use DWRITE_PARAGRAPH_ALIGNMENT_CENTER as MID;
        use DWRITE_PARAGRAPH_ALIGNMENT_NEAR as TOP;
        use DWRITE_TEXT_ALIGNMENT_CENTER as CENTER;
        use DWRITE_TEXT_ALIGNMENT_LEADING as START;
        use DWRITE_TEXT_ALIGNMENT_TRAILING as END;
        let f = &self.fonts;

        // Header: wordmark and live status share one baseline row.
        self.text(g, "goby", &f.mark, R::new(LEFT, 14.0, 200.0, 48.0), START, MID, rgb(INK))?;
        let status_color = if self.healthy { rgb(MUTED) } else { rgb(WARN) };
        self.text(g, &self.status, &f.small, R { l: 200.0, t: 14.0, r: RIGHT, b: 62.0 }, END, MID, status_color)?;

        // Scope: the signature. Raw voice faint, transformed voice bright, name underneath.
        self.fill(g, SCOPE, 16.0, rgb(SURFACE));
        self.outline(g, SCOPE, 16.0, rgb(EDGE));
        self.text(
            g,
            name_of(self.preset),
            &f.watermark,
            R { l: SCOPE.l + 24.0, t: SCOPE.b - 92.0, r: SCOPE.r - 24.0, b: SCOPE.b - 14.0 },
            START,
            DWRITE_PARAGRAPH_ALIGNMENT_FAR,
            rgb(WATERMARK),
        )?;
        let meta = match &self.engine {
            Some(_) => format!("48 кГц · {} мс", self.latency.round()),
            None => "нет звука".into(),
        };
        self.text(
            g,
            &meta,
            &f.small,
            R { l: SCOPE.l, t: SCOPE.t + 14.0, r: SCOPE.r - 20.0, b: SCOPE.t + 34.0 },
            END,
            TOP,
            rgb(MUTED),
        )?;
        self.line(g, &trace(&self.raw), rgb(FAINT), 1.4)?;
        self.line(g, &trace(&self.voiced), rgb(ACCENT), 2.2)?;

        // Presets as a typographic row; each glyph is the waveform it produces.
        let cell = PRESETS.w() / 6.0;
        for (i, p) in Preset::ALL.iter().enumerate() {
            let x = PRESETS.l + cell * i as f32;
            let glow = self.glow[i];
            let pts: Vec<(f32, f32)> =
                self.glyphs[i].iter().map(|(gx, gy)| (x + cell / 2.0 - 22.0 + gx, PRESETS.t + 8.0 + gy)).collect();
            self.line(g, &pts, mix(FAINT, ACCENT, glow), 1.7)?;
            self.text(
                g,
                name_of(*p),
                &f.preset,
                R::new(x, PRESETS.t + 32.0, cell, 28.0),
                CENTER,
                MID,
                mix(FAINT, INK, glow),
            )?;
        }

        let semis = self.settings.semitones;
        let tone = format!("{}{} пт", if semis > 0.0 { "+" } else { "" }, semis.round());
        self.ruler(g, TONE_Y, "Тон", &tone, (semis + 12.0) / 24.0, 0.5, 24, 6)?;
        let vol = self.settings.volume;
        self.ruler(g, VOLUME_Y, "Громкость", &format!("{} %", (vol * 100.0).round()), vol / 2.0, 0.0, 20, 5)?;

        // Routing.
        self.text(g, "Микрофон", &f.body, R::new(LEFT, MIC_Y, 130.0, COMBO_H), START, MID, rgb(MUTED))?;
        self.combo(g, Which::Mic, MIC_Y)?;
        self.text(g, "Куда отдавать", &f.body, R::new(LEFT, OUT_Y, 130.0, COMBO_H), START, MID, rgb(MUTED))?;
        self.combo(g, Which::Out, OUT_Y)?;

        let track = R::new(COMBO_X, MONITOR_Y + 2.0, 36.0, 20.0);
        self.fill(g, track, 10.0, mix(EDGE, ACCENT, self.knob));
        let kx = track.l + 10.0 + self.knob * 16.0;
        unsafe {
            g.brush.SetColor(&mix(MUTED, BASE, self.knob));
            g.rt.FillEllipse(&D2D1_ELLIPSE { point: v(kx, track.t + 10.0), radiusX: 7.0, radiusY: 7.0 }, &g.brush);
        }
        let monitor_color = if self.hover == Hit::Monitor { rgb(INK) } else { mix(MUTED, INK, 0.5) };
        self.text(
            g,
            "Слышать себя в наушниках",
            &f.body,
            R::new(COMBO_X + 48.0, MONITOR_Y, 260.0, 24.0),
            START,
            MID,
            monitor_color,
        )?;

        // Footer: one plain sentence and one way out.
        self.text(
            g,
            "CS2 слушает микрофон Windows по умолчанию.\nПоставьте там «CABLE Output».",
            &f.small,
            R { l: LEFT, t: FOOT_Y, r: 400.0, b: FOOT_Y + 36.0 },
            START,
            MID,
            rgb(MUTED),
        )?;
        let link_color = if self.hover == Hit::Link { rgb(ACCENT) } else { rgb(INK) };
        self.text(
            g,
            LINK_TEXT,
            &f.strong,
            R { l: 380.0, t: FOOT_Y, r: RIGHT - 16.0, b: FOOT_Y + 36.0 },
            END,
            MID,
            link_color,
        )?;
        let (ax, ay) = (RIGHT - 9.0, FOOT_Y + 14.0);
        self.line(g, &[(ax, ay + 8.0), (ax + 8.0, ay)], link_color, 1.6)?;
        self.line(g, &[(ax + 2.0, ay), (ax + 8.0, ay), (ax + 8.0, ay + 6.0)], link_color, 1.6)?;

        if let Some(which) = self.open {
            self.dropdown(g, which)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn ruler(
        &self,
        g: &Gfx,
        y: f32,
        label: &str,
        readout: &str,
        t: f32,
        origin: f32,
        intervals: usize,
        major: usize,
    ) -> Result<()> {
        let f = &self.fonts;
        self.text(
            g,
            label,
            &f.body,
            R { l: LEFT, t: y, r: 300.0, b: y + 20.0 },
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            rgb(MUTED),
        )?;
        self.text(
            g,
            readout,
            &f.readout,
            R { l: 300.0, t: y - 2.0, r: RIGHT, b: y + 22.0 },
            DWRITE_TEXT_ALIGNMENT_TRAILING,
            DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            rgb(INK),
        )?;
        let span = RIGHT - LEFT;
        for i in 0..=intervals {
            let x = LEFT + span * i as f32 / intervals as f32;
            let tall = i % major == 0;
            let top = if tall { y + 30.0 } else { y + 37.0 };
            self.line(g, &[(x, top), (x, y + 46.0)], rgb(FAINT), 2.0)?;
        }
        let (x0, x1) = (LEFT + span * origin, LEFT + span * t);
        self.line(g, &[(x0.min(x1), y + 54.0), (x0.max(x1), y + 54.0)], rgb(ACCENT), 3.0)?;
        let active = self.drag.is_some_and(|d| (d == Hit::Tone) == (y == TONE_Y))
            || matches!(self.hover, Hit::Tone if y == TONE_Y)
            || matches!(self.hover, Hit::Volume if y == VOLUME_Y);
        self.line(g, &[(x1, y + 27.0), (x1, y + 54.0)], rgb(ACCENT), if active { 5.0 } else { 4.0 })?;
        Ok(())
    }

    fn combo(&self, g: &Gfx, which: Which, y: f32) -> Result<()> {
        let r = R::new(COMBO_X, y, RIGHT - COMBO_X, COMBO_H);
        let hovered = self.hover == Hit::Combo(which) || self.open == Some(which);
        self.fill(g, r, 8.0, if hovered { mix(FIELD, EDGE, 0.6) } else { rgb(FIELD) });
        let name = match which {
            Which::Mic => self.inputs.get(self.input),
            Which::Out => self.outputs.get(self.output),
        }
        .map(|e| e.name.as_str())
        .unwrap_or("—");
        self.text(
            g,
            name,
            &self.fonts.field,
            R { l: r.l + 12.0, t: r.t, r: r.r - 34.0, b: r.b },
            DWRITE_TEXT_ALIGNMENT_LEADING,
            DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
            rgb(INK),
        )?;
        let (cx, cy) = (r.r - 18.0, r.t + COMBO_H / 2.0);
        let flip = if self.open == Some(which) { -1.0 } else { 1.0 };
        self.line(
            g,
            &[(cx - 4.5, cy - 2.0 * flip), (cx, cy + 2.5 * flip), (cx + 4.5, cy - 2.0 * flip)],
            rgb(MUTED),
            1.6,
        )
    }

    fn dropdown(&self, g: &Gfx, which: Which) -> Result<()> {
        let (r, shown) = self.list_rect(which);
        let (items, selected) = match which {
            Which::Mic => (&self.inputs, self.input),
            Which::Out => (&self.outputs, self.output),
        };
        unsafe {
            // A tight, directional shadow so the list reads as lifted off the form.
            g.brush.SetColor(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.35 });
            let s = R { l: r.l + 1.0, t: r.t + 3.0, r: r.r + 1.0, b: r.b + 4.0 };
            g.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: s.d2d(), radiusX: 10.0, radiusY: 10.0 }, &g.brush);
        }
        self.fill(g, r, 10.0, mix(FIELD, EDGE, 0.35));
        self.outline(g, r, 10.0, rgb(EDGE));
        for row in 0..shown {
            let i = row + self.scroll;
            let Some(item) = items.get(i) else { break };
            let ir = R::new(r.l + 4.0, r.t + 4.0 + row as f32 * ITEM_H, r.w() - 8.0, ITEM_H);
            if self.hover == Hit::Item(which, i) {
                self.fill(g, ir, 6.0, rgb(EDGE));
            }
            let color = if i == selected { rgb(ACCENT) } else { rgb(INK) };
            self.text(
                g,
                &item.name,
                &self.fonts.field,
                R { l: ir.l + 10.0, t: ir.t, r: ir.r - 10.0, b: ir.b },
                DWRITE_TEXT_ALIGNMENT_LEADING,
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                color,
            )?;
        }
        Ok(())
    }
}

/// Oscilloscope trace: lock onto a rising zero crossing so the wave stands still,
/// then squash through tanh so loud voices stay inside the frame.
fn trace(samples: &[f32; SCOPE_LEN]) -> Vec<(f32, f32)> {
    const SPAN: usize = 768;
    let start =
        (1..SCOPE_LEN - SPAN).rev().find(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0).unwrap_or(SCOPE_LEN - SPAN);
    let (l, r) = (SCOPE.l + 18.0, SCOPE.r - 18.0);
    let mid = SCOPE.t + SCOPE.h() * 0.46;
    (0..=192)
        .map(|k| {
            let s = samples[start + k * SPAN / 192 - if k == 192 { 1 } else { 0 }];
            (l + (r - l) * k as f32 / 192.0, mid - (s * 4.0).tanh() * SCOPE.h() * 0.36)
        })
        .collect()
}

fn make_fonts(dw: &IDWriteFactory) -> Result<Fonts> {
    unsafe {
        // Basteleur ships inside the exe; load it into a private collection.
        let dw5: IDWriteFactory5 = dw.cast()?;
        let loader = dw5.CreateInMemoryFontFileLoader()?;
        dw5.RegisterFontFileLoader(&loader)?;
        let file =
            loader.CreateInMemoryFontFileReference(&dw5, BASTELEUR.as_ptr() as _, BASTELEUR.len() as u32, None)?;
        let builder = dw5.CreateFontSetBuilder()?;
        builder.AddFontFile(&file)?;
        let set = builder.CreateFontSet()?;
        let display: IDWriteFontCollection = dw5.CreateFontCollectionFromFontSet(&set)?.cast()?;

        let ui = |size: f32, weight: DWRITE_FONT_WEIGHT| -> Result<IDWriteTextFormat> {
            dw.CreateTextFormat(
                w!("Segoe UI Variable Text"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("ru-ru"),
            )
        };
        let bast = |size: f32| -> Result<IDWriteTextFormat> {
            dw.CreateTextFormat(
                w!("Basteleur"),
                &display,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
        };
        let field = ui(13.0, DWRITE_FONT_WEIGHT_NORMAL)?;
        field.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
        let ellipsis = dw.CreateEllipsisTrimmingSign(&field)?;
        field.SetTrimming(
            &DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 },
            &ellipsis,
        )?;
        Ok(Fonts {
            body: ui(13.0, DWRITE_FONT_WEIGHT_NORMAL)?,
            strong: ui(13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
            small: ui(12.0, DWRITE_FONT_WEIGHT_NORMAL)?,
            readout: ui(15.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?,
            field,
            mark: bast(30.0)?,
            preset: bast(17.0)?,
            watermark: bast(58.0)?,
        })
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
                    app.status = format!("Ошибка отрисовки: {}", e.message());
                }
                let _ = ValidateRect(Some(hwnd), None);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                app.tick();
                if !IsIconic(hwnd).as_bool() {
                    app.shared.scope(&mut app.raw, &mut app.voiced);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                app.mouse = point(lp, app.dpi);
                if let Some(d) = app.drag {
                    app.ruler_value(d, app.mouse.0);
                }
                app.hover = app.hit(app.mouse);
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
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                app.mouse = point(lp, app.dpi);
                app.press(msg == WM_LBUTTONDBLCLK);
                app.hover = app.hit(app.mouse);
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
                app.wheel(((wp.0 >> 16) & 0xffff) as i16 > 0);
                app.hover = app.hit(app.mouse);
                LRESULT(0)
            }
            WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
                app.open = None;
                LRESULT(0)
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
                let cursor = if app.hover == Hit::None { IDC_ARROW } else { IDC_HAND };
                SetCursor(LoadCursorW(None, cursor).ok());
                LRESULT(1)
            }
            WM_SIZE => {
                if let Some(g) = &app.gfx {
                    let size = D2D_SIZE_U { width: (lp.0 & 0xffff) as u32, height: ((lp.0 >> 16) & 0xffff) as u32 };
                    let _ = g.rt.Resize(&size);
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.dpi = (wp.0 & 0xffff) as f32;
                app.gfx = None;
                let r = *(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                LRESULT(0)
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
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dpi);
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
            WINDOW_EX_STYLE(0),
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

        // Dark title bar in the app's own base colour.
        let dark: i32 = 1;
        let caption: u32 = (BASE & 0xff) << 16 | (BASE & 0xff00) | (BASE >> 16);
        let text: u32 = (INK & 0xff) << 16 | (INK & 0xff00) | (INK >> 16);
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(20), &dark as *const _ as _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(35), &caption as *const _ as _, 4);
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(36), &text as *const _ as _, 4);
        let app = Box::new(App::new(hwnd)?);
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
