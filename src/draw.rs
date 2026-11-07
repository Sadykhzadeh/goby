//! What the window is drawn with, whatever puts it on screen: rectangles, colours, the type
//! ramp, icons, and the `Canvas` that Direct2D (`gfx`) and the website's canvas implement.
//! All coordinates are DIPs.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

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
}

/// The Windows 11 type ramp (sizes in DIPs) plus the system icon font.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Font {
    Caption,
    Body,
    Strong,
    Subtitle,
    Title,
    Icon16,
    Icon12,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// Where the window's pixels go. Drawing does not fail midway: a backend that hits an
/// error keeps it and reports it when the frame ends.
pub trait Canvas {
    /// Pixels per inch of the surface, for pictures made to size.
    fn dpi(&self) -> f32;
    fn fill(&self, r: R, radius: f32, c: Color);
    /// A stroke drawn inside `r`, so a 1 px border sits on the pixel grid.
    fn stroke(&self, r: R, radius: f32, c: Color, width: f32);
    fn circle(&self, x: f32, y: f32, radius: f32, c: Color);
    /// A polyline with round caps and joins.
    fn line(&self, pts: &[(f32, f32)], c: Color, width: f32);
    /// One line of text, vertically centred in `r`, ending in an ellipsis if it is too long.
    fn text(&self, s: &str, f: Font, r: R, align: Align, c: Color);
    /// A glyph from `glyph`, centred in `r`.
    fn icon(&self, code: u16, f: Font, r: R, c: Color);
    fn measure(&self, s: &str, f: Font) -> f32;
    /// Draws the picture known as `key` into `r`, asking `make` for its premultiplied BGRA
    /// pixels (`px`×`px`) the first time. False when there is no picture to draw.
    fn picture(&self, key: &str, px: u32, make: &mut dyn FnMut() -> Option<Vec<u8>>, r: R) -> bool;
    fn clip(&self, r: R);
    fn unclip(&self);
    /// Draws what follows `dy` lower and at `opacity`, until `unshift`: a page coming in.
    fn shift(&self, dy: f32, opacity: f32);
    fn unshift(&self);
}

/// Icon code points from Segoe Fluent Icons (same in Segoe MDL2 Assets).
pub mod glyph {
    pub const MIC: u16 = 0xE720;
    pub const SPEAKERS: u16 = 0xE7F5;
    pub const HEADPHONE: u16 = 0xE7F6;
    pub const CHEVRON_DOWN: u16 = 0xE70D;
    pub const CHEVRON_UP: u16 = 0xE70E;
    pub const MORE: u16 = 0xE712;
    pub const CHECK: u16 = 0xE73E;
    pub const UPDATE: u16 = 0xE896;
    pub const OPEN: u16 = 0xE8A7;
    pub const CHAT: u16 = 0xE8BD;
    pub const ADD: u16 = 0xE710;
    pub const INFO: u16 = 0xE946;
    pub const REMOVE: u16 = 0xE711;
    pub const GLOBE: u16 = 0xE774;
    pub const VIDEO: u16 = 0xE714;
    pub const GAME: u16 = 0xE7FC;
    pub const SPEED: u16 = 0xEC4A;
    pub const VOLUME: u16 = 0xE767;
    pub const TONE: u16 = 0xE8D6;
    pub const GATE: u16 = 0xE9E9;
    pub const MIX: u16 = 0xE9D9;
    pub const MINIMIZE: u16 = 0xE921;
    pub const POWER: u16 = 0xE7E8;
    pub const KEYBOARD: u16 = 0xE765;
    pub const SETTINGS: u16 = 0xE713;
    pub const VOICES: u16 = 0xE76E;
    pub const APPS: u16 = 0xE71D;
    pub const STAR: u16 = 0xE734;
    pub const STAR_FILLED: u16 = 0xE735;

    /// The same icons as SVG path data on a 16 unit grid, for surfaces without the Segoe
    /// icon font (it only ships with Windows): (filled, path) parts, stroked 1 unit wide
    /// with round ends where not filled. Windows draws the font instead.
    #[cfg_attr(windows, allow(dead_code))]
    pub fn shape(code: u16) -> &'static [(bool, &'static str)] {
        match code {
            VOICES => &[
                (false, "M1.5 8a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0 -13 0zM5.6 9.6c1.3 1.6 3.5 1.6 4.8 0"),
                (true, "M5.5 6.4a0.5 0.5 0 1 0 1 0a0.5 0.5 0 1 0 -1 0zM9.5 6.4a0.5 0.5 0 1 0 1 0a0.5 0.5 0 1 0 -1 0z"),
            ],
            APPS => &[(
                false,
                "M2.4 2.5h1.7a0.4 0.4 0 0 1 0.4 0.4v1.7a0.4 0.4 0 0 1 -0.4 0.4h-1.7a0.4 0.4 0 0 1 -0.4 -0.4v-1.7a0.4 0.4 0 0 1 0.4 -0.4zM2.4 6.75h1.7a0.4 0.4 0 0 1 0.4 0.4v1.7a0.4 0.4 0 0 1 -0.4 0.4h-1.7a0.4 0.4 0 0 1 -0.4 -0.4v-1.7a0.4 0.4 0 0 1 0.4 -0.4zM2.4 11h1.7a0.4 0.4 0 0 1 0.4 0.4v1.7a0.4 0.4 0 0 1 -0.4 0.4h-1.7a0.4 0.4 0 0 1 -0.4 -0.4v-1.7a0.4 0.4 0 0 1 0.4 -0.4zM7 3.75h7M7 8h7M7 12.25h7",
            )],
            SETTINGS => &[(
                false,
                "M12.88 6.51L14.62 6.95L14.62 9.05L12.88 9.49L12.50 10.39L13.42 11.94L11.94 13.42L10.39 12.50L9.49 12.88L9.05 14.62L6.95 14.62L6.51 12.88L5.61 12.50L4.06 13.42L2.58 11.94L3.50 10.39L3.12 9.49L1.38 9.05L1.38 6.95L3.12 6.51L3.50 5.61L2.58 4.06L4.06 2.58L5.61 3.50L6.51 3.12L6.95 1.38L9.05 1.38L9.49 3.12L10.39 3.50L11.94 2.58L13.42 4.06L12.50 5.61zM5.8 8a2.2 2.2 0 1 0 4.4 0a2.2 2.2 0 1 0 -4.4 0z",
            )],
            TONE => &[(
                false,
                "M6 12V3.6l7.5-1.6v8.5M2.8 12a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0zM10.3 10.5a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0zM6 6.2l7.5-1.6",
            )],
            VOLUME => &[(false, "M2 6h2.6L8 3v10L4.6 10H2zM10.4 6.1a2.8 2.8 0 0 1 0 3.8M12.4 4.3a5.4 5.4 0 0 1 0 7.4")],
            HEADPHONE => &[(
                false,
                "M2.5 11.5V8.5a5.5 5.5 0 0 1 11 0v3M2.5 9.5h2v4.5h-1a1 1 0 0 1-1-1zM13.5 9.5h-2v4.5h1a1 1 0 0 0 1-1z",
            )],
            STAR => &[(
                false,
                "M8.00 2.00L9.70 6.25L14.28 6.56L10.76 9.50L11.88 13.94L8.00 11.50L4.12 13.94L5.24 9.50L1.72 6.56L6.30 6.25z",
            )],
            STAR_FILLED => &[(
                true,
                "M8.00 2.00L9.70 6.25L14.28 6.56L10.76 9.50L11.88 13.94L8.00 11.50L4.12 13.94L5.24 9.50L1.72 6.56L6.30 6.25z",
            )],
            MIC => &[(
                false,
                "M8 1.5h0a2.2 2.2 0 0 1 2.2 2.2v3.6a2.2 2.2 0 0 1 -2.2 2.2h0a2.2 2.2 0 0 1 -2.2 -2.2v-3.6a2.2 2.2 0 0 1 2.2 -2.2zM3.3 7.6a4.7 4.7 0 0 0 9.4 0M8 12.3v2.2",
            )],
            GATE => &[(
                false,
                "M1.5 5h7M11.6 5h2.9M1.5 11h2M6.6 11h7.9M8.4 5a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0zM3.4 11a1.6 1.6 0 1 0 3.2 0a1.6 1.6 0 1 0 -3.2 0z",
            )],
            MORE => &[(
                true,
                "M2.6 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0zM7.1 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0zM11.6 8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0z",
            )],
            SPEAKERS => &[(
                false,
                "M5.5 1.5h5a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1 -1.5 1.5h-5a1.5 1.5 0 0 1 -1.5 -1.5v-10a1.5 1.5 0 0 1 1.5 -1.5zM5.6 10a2.4 2.4 0 1 0 4.8 0a2.4 2.4 0 1 0 -4.8 0zM7.1 4.8a0.9 0.9 0 1 0 1.8 0a0.9 0.9 0 1 0 -1.8 0z",
            )],
            MIX => &[(
                false,
                "M4 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-8a2 2 0 0 1 -2 -2v-8a2 2 0 0 1 2 -2zM4 8.5h1.8l1.4-2.8 2 5 1.3-2.2H12",
            )],
            MINIMIZE => &[(false, "M3 8h10")],
            POWER => &[(false, "M8 1.8v5.6M4.6 3.9a5.4 5.4 0 1 0 6.8 0")],
            KEYBOARD => &[(
                false,
                "M3 4h10a1.5 1.5 0 0 1 1.5 1.5v5.5a1.5 1.5 0 0 1 -1.5 1.5h-10a1.5 1.5 0 0 1 -1.5 -1.5v-5.5a1.5 1.5 0 0 1 1.5 -1.5zM4 6.6h.6M6.5 6.6h.6M9 6.6h.6M11.5 6.6h.6M4 8.4h.6M11.5 8.4h.6M5.6 10.3h4.8",
            )],
            UPDATE => &[(false, "M8 2v9M4.5 7.5 8 11l3.5-3.5M3 14h10")],
            SPEED => &[
                (false, "M2.3 12a6 6 0 1 1 11.4 0M8.6 9.2l2.6-3.4"),
                (true, "M6.9 10a1.1 1.1 0 1 0 2.2 0a1.1 1.1 0 1 0 -2.2 0z"),
            ],
            INFO => &[
                (false, "M1.5 8a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0 -13 0zM8 7.3v4"),
                (true, "M7.4 5.1a0.6 0.6 0 1 0 1.2 0a0.6 0.6 0 1 0 -1.2 0z"),
            ],
            ADD => &[(false, "M8 2.5v11M2.5 8h11")],
            CHEVRON_DOWN => &[(false, "M4 6l4 4 4-4")],
            REMOVE => &[(false, "M4 4l8 8M12 4l-8 8")],
            CHECK => &[(false, "M3 8.6l3 3 7-7")],
            OPEN => {
                &[(false, "M9.2 2.5h4.3v4.3M13.5 2.5 8 8M12 9.5v3a1 1 0 0 1-1 1H3.5a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1h3")]
            }
            GAME => &[
                (
                    false,
                    "M5 4.5h6a3.4 3.4 0 0 1 3.3 4.2l-.6 2.9a1.7 1.7 0 0 1-3 .8L9.4 11H6.6l-1.3 1.4a1.7 1.7 0 0 1-3-.8l-.6-2.9A3.4 3.4 0 0 1 5 4.5zM5 6.6v2.8M3.6 8h2.8",
                ),
                (true, "M9.9 7.2a0.5 0.5 0 1 0 1 0a0.5 0.5 0 1 0 -1 0zM11.3 8.7a0.5 0.5 0 1 0 1 0a0.5 0.5 0 1 0 -1 0z"),
            ],
            CHAT => &[(
                false,
                "M3.5 2.5h9a2 2 0 0 1 2 2V10a2 2 0 0 1-2 2H7.5L4.3 14.5V12h-.8a2 2 0 0 1-2-2V4.5a2 2 0 0 1 2-2z",
            )],
            GLOBE => &[(
                false,
                "M1.5 8a6.5 6.5 0 1 0 13 0a6.5 6.5 0 1 0 -13 0zM5.3 8a2.7 6.5 0 1 0 5.4 0a2.7 6.5 0 1 0 -5.4 0zM1.5 8h13M2.4 4.8h11.2M2.4 11.2h11.2",
            )],
            VIDEO => &[(
                false,
                "M3 4h6a1.5 1.5 0 0 1 1.5 1.5v5a1.5 1.5 0 0 1 -1.5 1.5h-6a1.5 1.5 0 0 1 -1.5 -1.5v-5a1.5 1.5 0 0 1 1.5 -1.5zM10.5 7l4-2v6l-4-2",
            )],
            CHEVRON_UP => &[(false, "M4 10l4-4 4 4")],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_a_shape_for_the_web() {
        use glyph::*;
        let all = [
            MIC,
            SPEAKERS,
            HEADPHONE,
            CHEVRON_DOWN,
            CHEVRON_UP,
            MORE,
            CHECK,
            UPDATE,
            OPEN,
            CHAT,
            ADD,
            INFO,
            REMOVE,
            GLOBE,
            VIDEO,
            GAME,
            SPEED,
            VOLUME,
            TONE,
            GATE,
            MIX,
            MINIMIZE,
            POWER,
            KEYBOARD,
            SETTINGS,
            VOICES,
            APPS,
            STAR,
            STAR_FILLED,
        ];
        for code in all {
            assert!(!shape(code).is_empty(), "{code:X} has no shape");
        }
    }
}
