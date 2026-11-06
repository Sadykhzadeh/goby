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

/// The entry `step` places away from `current` in `list`, wrapping around. From outside the
/// list a step forward lands on its first entry and a step back on its last.
pub fn step_in<T: Copy + PartialEq>(list: &[T], current: Option<T>, step: isize) -> Option<T> {
    if list.is_empty() {
        return None;
    }
    let len = list.len() as isize;
    let at = match current.and_then(|c| list.iter().position(|x| *x == c)) {
        Some(pos) => (pos as isize + step).rem_euclid(len),
        None if step > 0 => 0,
        None => len - 1,
    };
    Some(list[at as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps_around_and_enters_from_either_end() {
        let list = [1, 2, 3, 4];
        assert_eq!(step_in(&list, Some(1), 1), Some(2));
        assert_eq!(step_in(&list, Some(4), 1), Some(1));
        assert_eq!(step_in(&list, Some(1), -1), Some(4));
        assert_eq!(step_in(&list, Some(0), 1), Some(1));
        assert_eq!(step_in(&list, Some(0), -1), Some(4));
        assert_eq!(step_in(&list, None, 1), Some(1));
        assert_eq!(step_in::<usize>(&[], Some(1), 1), None);
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
