//! The notification-area icon that keeps Goby within reach while its window is closed.

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_NONE, NIIF_RESPECT_QUIET_TIME, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NIN_SELECT, NINF_KEY, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, PCWSTR};

/// Sent to the window for clicks on the icon.
pub const WM_TRAY: u32 = WM_APP + 1;
/// Quits a running Goby even when closing its window would only hide it. The installer
/// sends it before replacing or removing the executable.
pub const WM_QUIT_GOBY: u32 = WM_APP + 2;

/// A click on the icon, and Enter or Space on it from the keyboard.
pub const SELECT: u32 = NIN_SELECT;
pub const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;

const ID: u32 = 1;

/// One line of the icon's menu; `None` draws a separator.
pub struct Item {
    pub label: Option<String>,
    pub checked: bool,
    pub default: bool,
}

/// `s` as a NUL-terminated wide string cut to fit `dst`.
fn copy(dst: &mut [u16], s: &str) {
    let wide: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..wide.len()].copy_from_slice(&wide);
    dst[wide.len()] = 0;
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: ID, ..Default::default() }
}

/// Puts the icon in the notification area; false if Explorer is not running.
pub fn add(hwnd: HWND, icon: HICON, tip: &str) -> bool {
    let mut d = data(hwnd);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = icon;
    copy(&mut d.szTip, tip);
    unsafe {
        if !Shell_NotifyIconW(NIM_ADD, &d).as_bool() {
            return false;
        }
        d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &d);
    }
    true
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    let mut d = data(hwnd);
    d.uFlags = NIF_TIP | NIF_SHOWTIP;
    copy(&mut d.szTip, tip);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

/// A notification from the icon; Windows keeps it quiet during a game or presentation.
pub fn notify(hwnd: HWND, title: &str, text: &str) {
    let mut d = data(hwnd);
    d.uFlags = NIF_INFO;
    d.dwInfoFlags = NIIF_NONE | NIIF_RESPECT_QUIET_TIME;
    copy(&mut d.szInfoTitle, title);
    copy(&mut d.szInfo, text);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data(hwnd));
    }
}

/// The icon's event, from the `lParam` of a [`WM_TRAY`] message.
pub fn event(lp: LPARAM) -> u32 {
    (lp.0 & 0xffff) as u32
}

/// Where the event happened, in screen pixels, from its `wParam`.
pub fn anchor(wp: WPARAM) -> POINT {
    POINT { x: (wp.0 & 0xffff) as i16 as i32, y: ((wp.0 >> 16) & 0xffff) as i16 as i32 }
}

/// Shows the menu and returns the index of the chosen item. It runs a modal loop that
/// sends messages back to the window, so no `&mut App` may be alive across it.
pub fn menu(hwnd: HWND, at: POINT, items: &[Item]) -> Option<usize> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (i, item) in items.iter().enumerate() {
            match &item.label {
                None => {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                }
                Some(label) => {
                    let flags = if item.checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                    let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(label.as_str()));
                    if item.default {
                        let _ = SetMenuDefaultItem(menu, (i + 1) as u32, 0);
                    }
                }
            }
        }
        // Without this the menu stays open after a click elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
        let id = TrackPopupMenuEx(menu, flags.0, at.x, at.y, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        (id.0 as usize).checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_tip_is_cut_and_ends_in_nul() {
        let mut d = data(HWND::default());
        copy(&mut d.szTip, &"x".repeat(500));
        assert_eq!(d.szTip[d.szTip.len() - 1], 0);
        assert!(d.szTip[..d.szTip.len() - 1].iter().all(|&c| c == 'x' as u16));

        copy(&mut d.szTip, "Goby: Минион");
        let end = d.szTip.iter().position(|&c| c == 0).unwrap();
        assert_eq!(String::from_utf16(&d.szTip[..end]).unwrap(), "Goby: Минион");
    }

    #[test]
    fn the_anchor_reads_negative_coordinates_on_a_left_monitor() {
        let wp = WPARAM(((200u32 << 16) | (-1500i16 as u16 as u32)) as usize);
        let p = anchor(wp);
        assert_eq!((p.x, p.y), (-1500, 200));
    }
}
