//! The app icon: a voice wave that swells in the middle. Drawn from code, so the executable
//! (via build.rs) and the window share one picture and no image files are needed.

use std::f32::consts::{PI, TAU};

/// Straight-alpha BGRA pixels, top row first, of the icon at `size`×`size` in colour `rgb`.
pub fn pixels(size: usize, rgb: u32) -> Vec<u32> {
    let s = size as f32;
    // Thicker at small sizes so the wave survives a 16 px taskbar slot.
    let half = (s / 14.0).max(1.1);
    // Keep the round line ends a pixel clear of the edge.
    let pad = (s * 0.08).max(half + 1.0);
    let points: Vec<(f32, f32)> = (0..=96)
        .map(|i| {
            let t = i as f32 / 96.0;
            let swell = (PI * t).sin().powf(0.8);
            (pad + (s - 2.0 * pad) * t, s / 2.0 - (t * 3.0 * TAU).sin() * swell * s * 0.36)
        })
        .collect();
    (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
            let d = points
                .windows(2)
                .map(|seg| {
                    let ((ax, ay), (bx, by)) = (seg[0], seg[1]);
                    let (dx, dy) = (bx - ax, by - ay);
                    let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
                    ((x - ax - t * dx).powi(2) + (y - ay - t * dy).powi(2)).sqrt()
                })
                .fold(f32::MAX, f32::min);
            let alpha = ((half + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
            if alpha == 0 { 0 } else { (alpha << 24) | (rgb & 0x00ff_ffff) }
        })
        .collect()
}

/// The same pixels with colour premultiplied by alpha, as GDI bitmaps want them.
#[allow(dead_code)] // build.rs includes this file too and has no use for it
pub fn premultiplied(size: usize, rgb: u32) -> Vec<u32> {
    pixels(size, rgb)
        .into_iter()
        .map(|p| {
            let a = p >> 24;
            let c = |shift: u32| ((p >> shift) & 0xff) * a / 255;
            (a << 24) | (c(16) << 16) | (c(8) << 8) | c(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_is_drawn_and_stays_inside() {
        for size in [16usize, 32, 256] {
            let px = pixels(size, 0xFFB900);
            let inked = px.iter().filter(|p| **p >> 24 > 0).count();
            assert!(inked > size, "{size}px icon is almost empty");
            // The outermost rows and columns stay clear, so nothing touches the edge.
            for i in 0..size {
                for (x, y) in [(i, 0), (i, size - 1), (0, i), (size - 1, i)] {
                    assert_eq!(px[y * size + x] >> 24, 0, "{size}px icon touches its edge at {x},{y}");
                }
            }
        }
    }

    #[test]
    fn premultiplying_scales_colour_by_alpha() {
        let straight = pixels(32, 0xFFFFFF);
        let pre = premultiplied(32, 0xFFFFFF);
        for (s, p) in straight.iter().zip(&pre) {
            assert_eq!(s >> 24, p >> 24);
            assert_eq!(p & 0xff, s >> 24, "white premultiplied equals its alpha");
        }
    }
}
