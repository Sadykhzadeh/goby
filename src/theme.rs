//! Windows 11 colour tokens for light or dark and an accent palette (the app reads both from
//! the system).
//! Values are WinUI's own (microsoft-ui-xaml Common_themeresources_any.xaml), as ARGB.

use crate::draw::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// The system accent palette: light3, light2, light1, accent, dark1, dark2, dark3 (RGB).
    palette: [u32; 7],
}

pub fn argb(v: u32) -> Color {
    let c = |s: u32| ((v >> s) & 0xff) as f32 / 255.0;
    Color { r: c(16), g: c(8), b: c(0), a: c(24) }
}

pub fn with_alpha(mut c: Color, a: f32) -> Color {
    c.a *= a;
    c
}

impl Theme {
    /// `palette` is the accent palette: light3, light2, light1, accent, dark1, dark2, dark3 (RGB).
    pub fn new(dark: bool, palette: [u32; 7]) -> Theme {
        Theme { dark, palette }
    }

    fn pick(&self, dark: u32, light: u32) -> Color {
        argb(if self.dark { dark } else { light })
    }

    pub fn text(&self) -> Color {
        self.pick(0xFFFFFFFF, 0xE4000000)
    }
    pub fn text_secondary(&self) -> Color {
        self.pick(0xC5FFFFFF, 0x9E000000)
    }
    pub fn text_tertiary(&self) -> Color {
        self.pick(0x87FFFFFF, 0x72000000)
    }
    pub fn card(&self) -> Color {
        self.pick(0x0DFFFFFF, 0xB3FFFFFF)
    }
    pub fn card_stroke(&self) -> Color {
        self.pick(0x19000000, 0x0F000000)
    }
    pub fn control_hover(&self) -> Color {
        self.pick(0x15FFFFFF, 0x80F9F9F9)
    }
    pub fn control_stroke(&self) -> Color {
        self.pick(0x12FFFFFF, 0x0F000000)
    }
    pub fn strong_stroke(&self) -> Color {
        self.pick(0x8BFFFFFF, 0x72000000)
    }
    /// Window background when Mica is unavailable (Windows 10, transparency off).
    pub fn solid_background(&self) -> Color {
        self.pick(0xFF202020, 0xFFF3F3F3)
    }
    /// Solid fill under the slider thumb and similar raised controls.
    pub fn control_solid(&self) -> Color {
        self.pick(0xFF454545, 0xFFFFFFFF)
    }
    /// Unchecked toggle track.
    pub fn control_alt(&self) -> Color {
        self.pick(0x19000000, 0x06000000)
    }

    /// Accent fill: light2 on dark, dark1 on light, as WinUI does.
    pub fn accent(&self) -> Color {
        argb(0xFF00_0000 | self.palette[if self.dark { 1 } else { 4 }])
    }
    pub fn on_accent(&self) -> Color {
        self.pick(0xFF000000, 0xFFFFFFFF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_unpacks_alpha_and_channels() {
        let c = argb(0x80FF0040);
        assert!((c.a - 0.502).abs() < 0.01 && c.r == 1.0 && c.g == 0.0 && (c.b - 0.251).abs() < 0.01);
    }

    #[test]
    fn primary_text_clears_the_solid_background_in_both_themes() {
        // WCAG contrast on the fallback background (Mica is close to it in value).
        fn lum(c: Color, bg: Color) -> f32 {
            let mixc = |a: f32, b: f32| a * c.a + b * (1.0 - c.a);
            let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            0.2126 * lin(mixc(c.r, bg.r)) + 0.7152 * lin(mixc(c.g, bg.g)) + 0.0722 * lin(mixc(c.b, bg.b))
        }
        for dark in [true, false] {
            let t = Theme { dark, palette: [0; 7] };
            let bg = t.solid_background();
            for (name, fg) in [("primary", t.text()), ("secondary", t.text_secondary())] {
                let (a, b) = (lum(fg, bg), lum(bg, bg));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{name} text in {} theme: {ratio:.2}:1", if dark { "dark" } else { "light" });
            }
        }
    }
}
