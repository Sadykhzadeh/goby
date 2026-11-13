//! Where the window opens: the place it was last hidden or closed at, if that is still
//! somewhere someone can reach it.

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONULL, MONITORINFO, MonitorFromRect};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, SPI_GETWORKAREA, SW_HIDE, SW_SHOWMAXIMIZED, SetWindowPlacement, SystemParametersInfoW,
    WINDOWPLACEMENT, WPF_RESTORETOMAXIMIZED,
};

use super::Win;
use crate::settings::Placement;

/// Left, top, right, bottom in screen pixels.
type Rect = (i32, i32, i32, i32);

/// How much of the window's top has to be on a monitor for it to be grabbed and dragged back.
const GRAB_W: i32 = 100;
const GRAB_H: i32 = 60;

/// The top of the window, the title bar and a little below it.
fn grab_area(p: &Placement) -> Rect {
    (p.x, p.y, p.x.saturating_add(p.w), p.y.saturating_add(GRAB_H.min(p.h)))
}

/// Whether a piece of the window's top at least `GRAB_W` by `GRAB_H` lies inside one of the
/// monitors' work areas. A window that is only partly on screen still counts; one with no
/// size, or whose monitor is gone, does not.
fn reachable(p: &Placement, work: &[Rect]) -> bool {
    if p.w < GRAB_W || p.h < GRAB_H {
        return false;
    }
    let (l, t, r, b) = grab_area(p);
    work.iter().any(|&(wl, wt, wr, wb)| r.min(wr) - l.max(wl) >= GRAB_W && b.min(wb) - t.max(wt) >= GRAB_H)
}

/// Window placements count from the primary monitor's work area, not from the screen's corner.
fn origin() -> (i32, i32) {
    let mut work = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut work as *mut RECT as _), Default::default());
    }
    (work.left, work.top)
}

/// Where the window is now, from `GetWindowPlacement`: its normal rectangle even when it
/// is maximised or minimised.
fn current(hwnd: HWND) -> Option<Placement> {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(hwnd, &mut wp).ok()? };
    let (ox, oy) = origin();
    let r = wp.rcNormalPosition;
    let maximized = wp.showCmd == SW_SHOWMAXIMIZED.0 as u32 || wp.flags.0 & WPF_RESTORETOMAXIMIZED.0 != 0;
    Some(Placement { x: r.left + ox, y: r.top + oy, w: r.right - r.left, h: r.bottom - r.top, maximized })
}

/// Puts the still hidden window at `p` and says whether it did. It stays hidden, and
/// maximising is left to the first show: setting a maximised placement would show it now.
pub(super) fn restore(hwnd: HWND, p: &Placement) -> bool {
    let (l, t, r, b) = grab_area(p);
    let grab = RECT { left: l, top: t, right: r, bottom: b };
    unsafe {
        let monitor = MonitorFromRect(&grab, MONITOR_DEFAULTTONULL);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if monitor.is_invalid() || !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let w = info.rcWork;
        if !reachable(p, &[(w.left, w.top, w.right, w.bottom)]) {
            return false;
        }
        let (ox, oy) = origin();
        let wp = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: SW_HIDE.0 as u32,
            ptMinPosition: POINT { x: -1, y: -1 },
            ptMaxPosition: POINT { x: -1, y: -1 },
            rcNormalPosition: RECT { left: p.x - ox, top: p.y - oy, right: p.x - ox + p.w, bottom: p.y - oy + p.h },
            ..Default::default()
        };
        SetWindowPlacement(hwnd, &wp).is_ok()
    }
}

impl Win {
    /// Notes where the window is, for the next start. Saved with the settings.
    pub(super) fn remember_window(&mut self) {
        // A window that was never shown since it started maximised-to-be is not maximised yet.
        let pending = self.host.start_maximized;
        let now = current(self.host.hwnd).map(|p| Placement { maximized: p.maximized || pending, ..p });
        self.settings.window = now.or(self.settings.window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: Rect = (0, 0, 1920, 1040);
    const LEFT: Rect = (-1920, 0, 0, 1040);

    fn at(x: i32, y: i32, w: i32, h: i32) -> Placement {
        Placement { x, y, w, h, maximized: false }
    }

    #[test]
    fn a_window_on_screen_is_reachable() {
        assert!(reachable(&at(100, 100, 1000, 720), &[PRIMARY]));
        assert!(reachable(&at(0, 0, 1920, 1040), &[PRIMARY]));
    }

    #[test]
    fn a_window_wholly_off_screen_is_not() {
        assert!(!reachable(&at(3000, 100, 1000, 720), &[PRIMARY]));
        assert!(!reachable(&at(100, 2000, 1000, 720), &[PRIMARY]));
        assert!(!reachable(&at(-1200, 100, 1000, 720), &[PRIMARY]));
        assert!(!reachable(&at(100, -800, 1000, 720), &[PRIMARY]));
    }

    #[test]
    fn a_window_hanging_off_an_edge_counts_while_enough_of_its_top_shows() {
        assert!(reachable(&at(1820, 100, 1000, 720), &[PRIMARY]), "exactly 100 px in");
        assert!(!reachable(&at(1821, 100, 1000, 720), &[PRIMARY]), "99 px in");
        assert!(reachable(&at(-900, 100, 1000, 720), &[PRIMARY]), "exactly 100 px in on the left");
        assert!(!reachable(&at(-901, 100, 1000, 720), &[PRIMARY]));
        assert!(reachable(&at(100, 980, 1000, 720), &[PRIMARY]), "exactly 60 px above the bottom");
        assert!(!reachable(&at(100, 981, 1000, 720), &[PRIMARY]), "the title bar is under the taskbar");
        assert!(!reachable(&at(100, -1, 1000, 720), &[PRIMARY]), "the title bar is above the screen");
    }

    #[test]
    fn a_monitor_that_is_gone_takes_its_windows_with_it() {
        let on_left = at(-1500, 100, 1000, 720);
        assert!(reachable(&on_left, &[LEFT, PRIMARY]));
        assert!(!reachable(&on_left, &[PRIMARY]));
        assert!(!reachable(&on_left, &[]));
    }

    #[test]
    fn negative_coordinates_on_a_left_monitor_are_fine() {
        assert!(reachable(&at(-1920, 0, 1000, 720), &[LEFT, PRIMARY]));
        assert!(reachable(&at(-1000, 0, 1000, 720), &[LEFT]));
        // Straddling the two monitors, with the title bar mostly on the left one.
        assert!(reachable(&at(-500, 10, 1000, 720), &[LEFT]));
        assert!(reachable(&at(-500, 10, 1000, 720), &[PRIMARY]));
    }

    #[test]
    fn a_window_with_no_size_is_never_reachable() {
        for (w, h) in [(0, 0), (0, 720), (1000, 0), (-1000, 720), (1000, -720), (99, 720), (1000, 59)] {
            assert!(!reachable(&at(100, 100, w, h), &[PRIMARY]), "{w}x{h}");
        }
    }

    #[test]
    fn huge_values_do_not_overflow() {
        assert!(!reachable(&at(i32::MAX, i32::MAX, i32::MAX, i32::MAX), &[PRIMARY]));
        assert!(!reachable(&at(i32::MIN, i32::MIN, i32::MAX, i32::MAX), &[PRIMARY]));
        assert!(reachable(&at(0, 0, i32::MAX, i32::MAX), &[PRIMARY]));
    }

    #[test]
    fn a_saved_value_that_is_broken_leaves_nothing_to_restore() {
        for bad in ["", "1,2,3", "x,y,w,h,0", "1,2,0,0,0", "1,2,-5,-5,1"] {
            assert_eq!(crate::settings::Settings::parse(&format!("window={bad}\n")).window, None, "{bad:?}");
        }
    }
}
