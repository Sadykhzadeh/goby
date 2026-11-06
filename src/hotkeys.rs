//! Keys that reach Goby while a game has the focus: switch the effect, step through voices.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey, VIRTUAL_KEY, VK_G,
    VK_NEXT, VK_PRIOR,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Toggle,
    Next,
    Previous,
}

const KEYS: [(Action, VIRTUAL_KEY); 3] =
    [(Action::Toggle, VK_G), (Action::Next, VK_NEXT), (Action::Previous, VK_PRIOR)];

fn modifiers() -> HOT_KEY_MODIFIERS {
    MOD_CONTROL | MOD_ALT | MOD_NOREPEAT
}

/// Ctrl+Alt+G, Ctrl+Alt+Page Down and Ctrl+Alt+Page Up. False if another program holds
/// any of them; the others still work.
pub fn register(hwnd: HWND) -> bool {
    let mut all = true;
    for (id, (_, key)) in KEYS.iter().enumerate() {
        all &= unsafe { RegisterHotKey(Some(hwnd), id as i32 + 1, modifiers(), key.0 as u32) }.is_ok();
    }
    all
}

pub fn unregister(hwnd: HWND) {
    for id in 1..=KEYS.len() {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id as i32);
        }
    }
}

/// The action of a `WM_HOTKEY` with this id.
pub fn action(id: usize) -> Option<Action> {
    KEYS.get(id.checked_sub(1)?).map(|(a, _)| *a)
}

/// The voice `step` places away from `voice` in a catalog of `len`, wrapping around and
/// skipping the first entry, Off, which the toggle already covers.
pub fn cycle(voice: usize, step: isize, len: usize) -> usize {
    if len < 2 {
        return voice;
    }
    let count = (len - 1) as isize;
    let pos = voice.saturating_sub(1) as isize;
    let from = if voice == 0 && step > 0 { -1 } else { pos };
    1 + (from + step).rem_euclid(count) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_around_and_never_lands_on_off() {
        assert_eq!(cycle(1, 1, 5), 2);
        assert_eq!(cycle(4, 1, 5), 1);
        assert_eq!(cycle(1, -1, 5), 4);
        assert_eq!(cycle(0, 1, 5), 1);
        assert_eq!(cycle(0, -1, 5), 4);
        assert_eq!(cycle(0, 1, 1), 0);
    }

    #[test]
    fn every_registered_id_has_an_action() {
        assert_eq!(action(0), None);
        assert_eq!(action(1), Some(Action::Toggle));
        assert_eq!(action(2), Some(Action::Next));
        assert_eq!(action(3), Some(Action::Previous));
        assert_eq!(action(4), None);
    }
}
