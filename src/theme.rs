//! Windows 11 colour tokens, following the system light/dark setting and accent colour.
//! Values are WinUI's own (microsoft-ui-xaml Common_themeresources_any.xaml), as ARGB.

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    /// The system accent palette: light3, light2, light1, accent, dark1, dark2, dark3 (RGB).
    palette: [u32; 7],
}

pub fn argb(v: u32) -> D2D1_COLOR_F {
    let c = |s: u32| ((v >> s) & 0xff) as f32 / 255.0;
    D2D1_COLOR_F { r: c(16), g: c(8), b: c(0), a: c(24) }
}

pub fn with_alpha(mut c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    c.a *= a;
    c
}

impl Theme {
    /// Reads the current theme and accent; falls back to dark with the default blue accent.
    pub fn system() -> Theme {
        let mut light: u32 = 0;
        let mut size = 4u32;
        let dark = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                w!("AppsUseLightTheme"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut light as *mut u32 as _),
                Some(&mut size),
            )
        }
        .is_err()
            || light == 0;

        let mut raw = [0u8; 32];
        let mut size = raw.len() as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent"),
                w!("AccentPalette"),
                RRF_RT_REG_BINARY,
                None,
                Some(raw.as_mut_ptr() as _),
                Some(&mut size),
            )
        }
        .is_ok();
        // Bytes are R, G, B, A per entry.
        let palette = if ok && size >= 28 {
            std::array::from_fn(|i| (raw[i * 4] as u32) << 16 | (raw[i * 4 + 1] as u32) << 8 | raw[i * 4 + 2] as u32)
        } else {
            [0x99EBFF, 0x60CDFF, 0x0093F9, 0x005FB8, 0x003E92, 0x001968, 0x000A3D]
        };
        Theme { dark, palette }
    }

    fn pick(&self, dark: u32, light: u32) -> D2D1_COLOR_F {
        argb(if self.dark { dark } else { light })
    }

    pub fn text(&self) -> D2D1_COLOR_F {
        self.pick(0xFFFFFFFF, 0xE4000000)
    }
    pub fn text_secondary(&self) -> D2D1_COLOR_F {
        self.pick(0xC5FFFFFF, 0x9E000000)
    }
    pub fn text_tertiary(&self) -> D2D1_COLOR_F {
        self.pick(0x87FFFFFF, 0x72000000)
    }
    pub fn card(&self) -> D2D1_COLOR_F {
        self.pick(0x0DFFFFFF, 0xB3FFFFFF)
    }
    pub fn card_stroke(&self) -> D2D1_COLOR_F {
        self.pick(0x19000000, 0x0F000000)
    }
    pub fn control_hover(&self) -> D2D1_COLOR_F {
        self.pick(0x15FFFFFF, 0x80F9F9F9)
    }
    pub fn control_stroke(&self) -> D2D1_COLOR_F {
        self.pick(0x12FFFFFF, 0x0F000000)
    }
    pub fn strong_stroke(&self) -> D2D1_COLOR_F {
        self.pick(0x8BFFFFFF, 0x72000000)
    }
    /// Window background when Mica is unavailable (Windows 10, transparency off).
    pub fn solid_background(&self) -> D2D1_COLOR_F {
        self.pick(0xFF202020, 0xFFF3F3F3)
    }
    /// Solid fill under the slider thumb and similar raised controls.
    pub fn control_solid(&self) -> D2D1_COLOR_F {
        self.pick(0xFF454545, 0xFFFFFFFF)
    }
    /// Unchecked toggle track.
    pub fn control_alt(&self) -> D2D1_COLOR_F {
        self.pick(0x19000000, 0x06000000)
    }

    /// Accent fill: light2 on dark, dark1 on light, as WinUI does.
    pub fn accent(&self) -> D2D1_COLOR_F {
        argb(0xFF00_0000 | self.palette[if self.dark { 1 } else { 4 }])
    }
    pub fn on_accent(&self) -> D2D1_COLOR_F {
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
        fn lum(c: D2D1_COLOR_F, bg: D2D1_COLOR_F) -> f32 {
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
