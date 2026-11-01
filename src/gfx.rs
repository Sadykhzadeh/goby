//! Drawing: Direct2D 1.1 into a DirectComposition swap chain with alpha, so the system Mica
//! backdrop shows through wherever the window is transparent. All coordinates are DIPs.

use std::mem::ManuallyDrop;

use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device,
};
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::{BOOL, Interface, PCWSTR, Result, w};
use windows_numerics::Vector2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub l: f32,
    pub t: f32,
    pub r: f32,
    pub b: f32,
}

impl R {
    pub const fn new(l: f32, t: f32, w: f32, h: f32) -> R {
        R { l, t, r: l + w, b: t + h }
    }
    pub fn w(&self) -> f32 {
        self.r - self.l
    }
    pub fn h(&self) -> f32 {
        self.b - self.t
    }
    pub fn has(&self, (x, y): (f32, f32)) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    pub fn inset(&self, d: f32) -> R {
        R { l: self.l + d, t: self.t + d, r: self.r - d, b: self.b - d }
    }
    pub fn intersects(&self, o: &R) -> bool {
        self.l < o.r && o.l < self.r && self.t < o.b && o.t < self.b
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.l, top: self.t, right: self.r, bottom: self.b }
    }
}

/// The Windows 11 type ramp (sizes in DIPs) plus the system icon font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Font {
    Caption,
    Body,
    Strong,
    Subtitle,
    Icon16,
    Icon12,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
}

struct Fonts {
    caption: IDWriteTextFormat,
    body: IDWriteTextFormat,
    strong: IDWriteTextFormat,
    subtitle: IDWriteTextFormat,
    icon16: IDWriteTextFormat,
    icon12: IDWriteTextFormat,
}

pub struct Gfx {
    d2d: ID2D1Factory1,
    dw: IDWriteFactory,
    dc: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    _composition: (IDCompositionDevice, IDCompositionTarget, IDCompositionVisual),
    brush: ID2D1SolidColorBrush,
    round: ID2D1StrokeStyle,
    fonts: Fonts,
    pub dpi: f32,
}

fn family_exists(dw: &IDWriteFactory, name: PCWSTR) -> bool {
    unsafe {
        let mut collection = None;
        if dw.GetSystemFontCollection(&mut collection, false).is_err() {
            return false;
        }
        let (mut index, mut exists) = (0u32, BOOL(0));
        collection.is_some_and(|c| c.FindFamilyName(name, &mut index, &mut exists).is_ok() && exists.as_bool())
    }
}

fn make_fonts(dw: &IDWriteFactory) -> Result<Fonts> {
    unsafe {
        // Windows 11 ships Segoe UI Variable and Segoe Fluent Icons; Windows 10 has their
        // predecessors with the same metrics and code points.
        let variable = family_exists(dw, w!("Segoe UI Variable Text"));
        let (text, display) = if variable {
            (w!("Segoe UI Variable Text"), w!("Segoe UI Variable Display"))
        } else {
            (w!("Segoe UI"), w!("Segoe UI"))
        };
        let icons = if family_exists(dw, w!("Segoe Fluent Icons")) {
            w!("Segoe Fluent Icons")
        } else {
            w!("Segoe MDL2 Assets")
        };
        let format = |family: PCWSTR, weight: DWRITE_FONT_WEIGHT, size: f32| -> Result<IDWriteTextFormat> {
            let f = dw.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!(""),
            )?;
            // Single lines everywhere; anything too long ends in an ellipsis, never a clip.
            f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            let sign = dw.CreateEllipsisTrimmingSign(&f)?;
            f.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                },
                &sign,
            )?;
            Ok(f)
        };
        Ok(Fonts {
            caption: format(text, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
            body: format(text, DWRITE_FONT_WEIGHT_NORMAL, 14.0)?,
            strong: format(text, DWRITE_FONT_WEIGHT_SEMI_BOLD, 14.0)?,
            subtitle: format(display, DWRITE_FONT_WEIGHT_SEMI_BOLD, 20.0)?,
            icon16: format(icons, DWRITE_FONT_WEIGHT_NORMAL, 16.0)?,
            icon12: format(icons, DWRITE_FONT_WEIGHT_NORMAL, 12.0)?,
        })
    }
}

impl Gfx {
    pub fn new(hwnd: HWND, width_px: u32, height_px: u32, dpi: f32) -> Result<Gfx> {
        unsafe {
            let mut d3d: Option<ID3D11Device> = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut d3d),
                None,
                None,
            )?;
            let dxgi: IDXGIDevice = d3d.unwrap().cast()?;
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: width_px.max(1),
                Height: height_px.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap = factory.CreateSwapChainForComposition(&dxgi, &desc, None)?;

            let d2d: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dc = d2d.CreateDevice(&dxgi)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;

            let composition: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = composition.CreateTargetForHwnd(hwnd, true)?;
            let visual = composition.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            composition.Commit()?;

            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let round = d2d.cast::<ID2D1Factory>()?.CreateStrokeStyle(
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
            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let fonts = make_fonts(&dw)?;
            let gfx = Gfx { d2d, dw, dc, swap, _composition: (composition, target, visual), brush, round, fonts, dpi };
            gfx.bind_target()?;
            Ok(gfx)
        }
    }

    fn bind_target(&self) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: ManuallyDrop::new(None),
            };
            let bitmap = self.dc.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.dc.SetTarget(&bitmap);
            self.dc.SetDpi(self.dpi, self.dpi);
            self.dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        }
        Ok(())
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32, dpi: f32) -> Result<()> {
        self.dpi = dpi;
        unsafe {
            self.dc.SetTarget(None);
            self.swap.ResizeBuffers(
                0,
                width_px.max(1),
                height_px.max(1),
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        self.bind_target()
    }

    pub fn begin(&self, background: D2D1_COLOR_F) {
        unsafe {
            self.dc.BeginDraw();
            self.dc.Clear(Some(&background));
        }
    }

    /// Finishes the frame and hands it to the compositor. Errors mean the device is gone.
    pub fn end(&self) -> Result<()> {
        unsafe {
            self.dc.EndDraw(None, None)?;
            self.swap.Present(1, DXGI_PRESENT(0)).ok()
        }
    }

    pub fn fill(&self, r: R, radius: f32, c: D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(&c);
            self.dc.FillRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

    /// A stroke drawn inside `r`, so a 1 px border sits on the pixel grid.
    pub fn stroke(&self, r: R, radius: f32, c: D2D1_COLOR_F, width: f32) {
        unsafe {
            self.brush.SetColor(&c);
            let half = r.inset(width / 2.0);
            self.dc.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT { rect: half.d2d(), radiusX: radius, radiusY: radius },
                &self.brush,
                width,
                None,
            );
        }
    }

    pub fn circle(&self, x: f32, y: f32, radius: f32, c: D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(&c);
            self.dc.FillEllipse(
                &D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: radius, radiusY: radius },
                &self.brush,
            );
        }
    }

    pub fn line(&self, pts: &[(f32, f32)], c: D2D1_COLOR_F, width: f32) -> Result<()> {
        if pts.len() < 2 {
            return Ok(());
        }
        unsafe {
            let geo = self.d2d.CreatePathGeometry()?;
            let sink = geo.Open()?;
            sink.BeginFigure(Vector2 { X: pts[0].0, Y: pts[0].1 }, D2D1_FIGURE_BEGIN_HOLLOW);
            for p in &pts[1..] {
                sink.AddLine(Vector2 { X: p.0, Y: p.1 });
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            self.brush.SetColor(&c);
            self.dc.DrawGeometry(&geo, &self.brush, width, &self.round);
        }
        Ok(())
    }

    fn format(&self, f: Font) -> &IDWriteTextFormat {
        match f {
            Font::Caption => &self.fonts.caption,
            Font::Body => &self.fonts.body,
            Font::Strong => &self.fonts.strong,
            Font::Subtitle => &self.fonts.subtitle,
            Font::Icon16 => &self.fonts.icon16,
            Font::Icon12 => &self.fonts.icon12,
        }
    }

    /// One line of text, vertically centred in `r`.
    pub fn text(&self, s: &str, f: Font, r: R, align: Align, c: D2D1_COLOR_F) -> Result<()> {
        unsafe {
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.dw.CreateTextLayout(&wide, self.format(f), r.w().max(1.0), r.h().max(1.0))?;
            layout.SetTextAlignment(match align {
                Align::Start => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::End => DWRITE_TEXT_ALIGNMENT_TRAILING,
            })?;
            layout.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            self.brush.SetColor(&c);
            self.dc.DrawTextLayout(Vector2 { X: r.l, Y: r.t }, &layout, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        Ok(())
    }

    /// A glyph from the system icon font (Segoe Fluent Icons code points).
    pub fn icon(&self, code: u16, f: Font, r: R, c: D2D1_COLOR_F) -> Result<()> {
        self.text(&String::from_utf16_lossy(&[code]), f, r, Align::Center, c)
    }

    pub fn measure(&self, s: &str, f: Font) -> Result<f32> {
        unsafe {
            let wide: Vec<u16> = s.encode_utf16().collect();
            let layout = self.dw.CreateTextLayout(&wide, self.format(f), 4096.0, 256.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            Ok(m.width)
        }
    }

    pub fn clip(&self, r: R) {
        unsafe { self.dc.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) }
    }

    pub fn unclip(&self) {
        unsafe { self.dc.PopAxisAlignedClip() }
    }
}

/// Icon code points from Segoe Fluent Icons (same in Segoe MDL2 Assets).
pub mod glyph {
    pub const MIC: u16 = 0xE720;
    pub const SPEAKERS: u16 = 0xE7F5;
    pub const HEADPHONE: u16 = 0xE7F6;
    pub const CHEVRON_DOWN: u16 = 0xE70D;
    pub const OPEN: u16 = 0xE8A7;
    pub const CHECK: u16 = 0xE73E;
    pub const RENAME: u16 = 0xE8AC;
    pub const GAME: u16 = 0xE7FC;
    pub const SPEED: u16 = 0xEC4A;
    pub const VOLUME: u16 = 0xE767;
    pub const TONE: u16 = 0xE8D6;
    pub const GATE: u16 = 0xE9E9;
    pub const MIX: u16 = 0xE9D9;
}
