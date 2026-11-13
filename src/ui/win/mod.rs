//! The app's window: Win32 with a Mica backdrop and native menus, drawn by `gfx`. It hands
//! its messages to the shared `App` and answers the `Host` calls through `Desktop`.

use std::time::Duration;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, PCSTR, PCWSTR, PWSTR, Result, w};

use super::{App, HEIGHT, MIN_H, MIN_W, Popup, WIDTH};
use crate::draw::Color;
use crate::gfx::Gfx;
use crate::host::{Host, Hotkey, Key};
use crate::lang::tr;
use crate::settings::Settings;
use crate::theme::Theme;
use crate::{hotkeys, icon, tray};

mod background;
mod desktop;
mod placement;
mod updates;

use desktop::Desktop;

type Win = App<Desktop>;

const WS_EX_NOREDIRECTIONBITMAP: WINDOW_EX_STYLE = WINDOW_EX_STYLE(0x0020_0000);

/// Shows a menu from the window at its place under the card, with no borrow of the window
/// state alive: the menu runs a modal loop that sends timers and paints back to wndproc.
fn show_popup(p: &Popup, hwnd: HWND, dpi: f32) -> Option<String> {
    const BROWSE: usize = 10_000;
    let scale = dpi / 96.0;
    let mut at = POINT { x: (p.at.0 * scale) as i32, y: (p.at.1 * scale) as i32 };
    let chosen = unsafe {
        let _ = ClientToScreen(hwnd, &mut at);
        let menu = CreatePopupMenu().ok()?;
        for (i, label) in p.labels.iter().enumerate() {
            let flags = if p.checked == Some(i) { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(label.as_str()));
        }
        if let Some((label, _)) = &p.browse {
            if !p.labels.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            let _ = AppendMenuW(menu, MF_STRING, BROWSE, &HSTRING::from(label.as_str()));
        }
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_LEFTALIGN | TPM_TOPALIGN;
        let id = TrackPopupMenuEx(menu, flags.0, at.x, at.y, hwnd, None);
        let _ = DestroyMenu(menu);
        id.0 as usize
    };
    match chosen {
        0 => None,
        BROWSE => browse_exe(hwnd, &p.browse.as_ref()?.1),
        i => p.keys.get(i - 1).cloned(),
    }
}

#[repr(C)]
struct OpenFileName {
    struct_size: u32,
    owner: HWND,
    instance: HINSTANCE,
    filter: PCWSTR,
    custom_filter: PWSTR,
    max_custom_filter: u32,
    filter_index: u32,
    file: PWSTR,
    max_file: u32,
    file_title: PWSTR,
    max_file_title: u32,
    initial_dir: PCWSTR,
    title: PCWSTR,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_ext: PCWSTR,
    custom_data: LPARAM,
    hook: *mut std::ffi::c_void,
    template_name: PCWSTR,
    reserved: *mut std::ffi::c_void,
    reserved2: u32,
    flags_ex: u32,
}

#[link(name = "comdlg32", kind = "raw-dylib")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> BOOL;
}

/// The standard Open dialog, filtered to programs; returns the lower-case exe name.
fn browse_exe(owner: HWND, title: &str) -> Option<String> {
    let mut file = [0u16; 1024];
    let filter: Vec<u16> = format!("{}\0*.exe\0\0", tr!("programs.menu.filter")).encode_utf16().collect();
    let title = HSTRING::from(title);
    let mut ofn = OpenFileName {
        struct_size: size_of::<OpenFileName>() as u32,
        owner,
        instance: HINSTANCE::default(),
        filter: PCWSTR(filter.as_ptr()),
        custom_filter: PWSTR::null(),
        max_custom_filter: 0,
        filter_index: 1,
        file: PWSTR(file.as_mut_ptr()),
        max_file: file.len() as u32,
        file_title: PWSTR::null(),
        max_file_title: 0,
        initial_dir: PCWSTR::null(),
        title: PCWSTR(title.as_ptr()),
        // OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
        flags: 0x0008_0000 | 0x1000 | 0x0800 | 0x0008,
        file_offset: 0,
        file_extension: 0,
        default_ext: PCWSTR::null(),
        custom_data: LPARAM(0),
        hook: std::ptr::null_mut(),
        template_name: PCWSTR::null(),
        reserved: std::ptr::null_mut(),
        reserved2: 0,
        flags_ex: 0,
    };
    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None;
    }
    let len = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..len]);
    let name = path.rsplit('\\').next()?.to_ascii_lowercase();
    name.ends_with(".exe").then_some(name)
}

/// Lets Win32 menus follow the dark theme. uxtheme exports these by ordinal only; they have
/// been stable since Windows 10 1903 and are what Windows' own apps use for dark menus.
fn allow_dark_menus(hwnd: HWND) {
    unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set_preferred_app_mode(1); // AllowDark: follow the system setting
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(133 as *const u8)) {
            let allow_for_window: extern "system" fn(HWND, BOOL) -> BOOL = std::mem::transmute(f);
            let _ = allow_for_window(hwnd, true.into());
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let flush_menu_themes: extern "system" fn() = std::mem::transmute(f);
            flush_menu_themes();
        }
    }
}

/// Reads the current theme and accent; falls back to dark with the default blue accent.
fn system_theme() -> Theme {
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
    Theme::new(dark, palette)
}

impl Win {
    fn paint(&mut self) -> Result<()> {
        if self.host.gfx.is_none() {
            let mut rc = RECT::default();
            unsafe { GetClientRect(self.host.hwnd, &mut rc)? };
            self.host.gfx = Some(Gfx::new(self.host.hwnd, rc.right as u32, rc.bottom as u32, self.dpi)?);
        }
        let g = self.host.gfx.take().unwrap();
        let background = if self.host.mica { Color::default() } else { self.theme.solid_background() };
        g.begin(background);
        self.draw(&g);
        let drawn = g.take_error();
        let presented = g.end();
        self.host.gfx = Some(g);
        drawn?;
        if presented.is_err() {
            // Device lost (driver update, GPU reset): rebuild on the next frame.
            self.host.gfx = None;
        }
        self.dirty = false;
        Ok(())
    }

    /// Ticks once a frame while the window can be seen. Otherwise only the device retry,
    /// program pinning and the tray tip need the timer, and twice a second is plenty.
    fn pace(&mut self, shown: bool) {
        let ms = if shown { 33 } else { 500 };
        if ms != self.host.pace {
            self.host.pace = ms;
            unsafe { SetTimer(Some(self.host.hwnd), 1, ms, None) };
        }
    }

    /// True while a fullscreen window of another app (a game) covers our monitor.
    fn covered(&self) -> bool {
        let hwnd = self.host.hwnd;
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_invalid() || fg == hwnd {
                return false;
            }
            let mut wr = RECT::default();
            if GetWindowRect(fg, &mut wr).is_err() {
                return false;
            }
            let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
            if monitor != MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) {
                return false;
            }
            let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            GetMonitorInfoW(monitor, &mut info).as_bool() && info.rcMonitor == wr
        }
    }

    fn apply_frame(&mut self) {
        let hwnd = self.host.hwnd;
        unsafe {
            let dark = BOOL::from(self.theme.dark);
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as _, 4);
            let backdrop = DWMSBT_MAINWINDOW;
            let size = size_of_val(&backdrop) as u32;
            let mica = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &backdrop as *const _ as _, size).is_ok();
            self.host.mica = mica;
            if mica {
                let all = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
                self.host.mica = DwmExtendFrameIntoClientArea(hwnd, &all).is_ok();
            }
            for (kind, size) in [(ICON_BIG, 32), (ICON_SMALL, 16)] {
                if let Some(icon) = make_icon(size, icon::RGB) {
                    let icon = LPARAM(icon.0 as isize);
                    let old = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(icon));
                    if old.0 != 0 {
                        let _ = DestroyIcon(HICON(old.0 as _));
                    }
                }
            }
        }
        allow_dark_menus(hwnd);
    }
}

/// The window icon, identical to the executable's so the title bar matches the taskbar.
fn make_icon(size: i32, rgb: u32) -> Option<HICON> {
    let px = icon::premultiplied(size as usize, rgb);
    unsafe {
        let bitmap = CreateBitmap(size, size, 1, 32, Some(px.as_ptr() as _));
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: bitmap };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// Windows' "Animation effects" switch (Settings > Accessibility > Visual effects).
fn animations_on() -> bool {
    let mut on = BOOL(1);
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut BOOL as _), Default::default())
    };
    ok.is_err() || on.as_bool()
}

/// The message Explorer broadcasts when the taskbar comes back after a restart.
fn taskbar_created() -> u32 {
    static ID: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *ID.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

unsafe fn app_of(hwnd: HWND) -> Option<&'static mut Win> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Win;
    unsafe { p.as_mut() }
}

fn point(lp: LPARAM, dpi: f32) -> (f32, f32) {
    let x = (lp.0 & 0xffff) as i16 as f32;
    let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
    (x * 96.0 / dpi, y * 96.0 / dpi)
}

fn key_of(vk: u16) -> Option<Key> {
    Some(match VIRTUAL_KEY(vk) {
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_ESCAPE => Key::Escape,
        VK_BACK => Key::Back,
        _ => return None,
    })
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(app) = app_of(hwnd) else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_PAINT => {
                if let Err(e) = app.paint() {
                    app.status = e.message();
                    app.healthy = false;
                }
                let _ = ValidateRect(Some(hwnd), None);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_TIMER => {
                app.shown = IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() && !app.covered();
                app.tick();
                app.update_tip();
                if app.dirty && app.shown {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                app.pace(app.shown);
                LRESULT(0)
            }
            // Back from the tray or the taskbar: frames again from the next tick, not the
            // next slow one.
            WM_SHOWWINDOW if wp.0 != 0 => {
                app.pace(true);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOUSEMOVE => {
                app.moved(point(lp, app.dpi));
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
                app.left();
                LRESULT(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                if let Some(popup) = app.down(point(lp, app.dpi), msg == WM_LBUTTONDBLCLK) {
                    // `app` is not used across the menu's modal loop; it is fetched again after.
                    let chosen = show_popup(&popup, hwnd, app.dpi);
                    let Some(app) = app_of(hwnd) else { return LRESULT(0) };
                    app.menu_done(popup.pick, chosen);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                app.right_click(point(lp, app.dpi));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                app.up();
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let mut p = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                let at = (p.x as f32 * 96.0 / app.dpi, p.y as f32 * 96.0 / app.dpi);
                app.wheel(at, ((wp.0 >> 16) & 0xffff) as i16 as f32 / 120.0);
                LRESULT(0)
            }
            WM_KEYDOWN if key_of(wp.0 as u16).is_some_and(|k| app.key(k)) => LRESULT(0),
            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
                let (w, h) = window_size(MIN_W, MIN_H, app.dpi as u32, style);
                info.ptMinTrackSize = POINT { x: w, y: h };
                LRESULT(0)
            }
            WM_SIZE => {
                let (w, h) = ((lp.0 & 0xffff) as u32, ((lp.0 >> 16) & 0xffff) as u32);
                if w > 0 && h > 0 {
                    let scale = app.dpi / 96.0;
                    app.resize(w as f32 / scale, h as f32 / scale);
                }
                if wp.0 as u32 == SIZE_MINIMIZED {
                    // Give the GPU memory back while we sit in the taskbar.
                    app.host.gfx = None;
                } else {
                    app.pace(true);
                    if let Some(g) = &mut app.host.gfx
                        && g.resize(w, h, app.dpi).is_err()
                    {
                        app.host.gfx = None;
                    }
                }
                app.dirty = true;
                LRESULT(0)
            }
            WM_DPICHANGED => {
                app.dpi = (wp.0 & 0xffff) as f32;
                app.host.gfx = None;
                let r = *(lp.0 as *const RECT);
                let (w, h) = (r.right - r.left, r.bottom - r.top);
                let _ = SetWindowPos(hwnd, None, r.left, r.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // Light/dark or accent changed in Windows settings: follow it live.
                let theme = system_theme();
                if theme != app.theme {
                    app.theme = theme;
                    app.apply_frame();
                    app.dirty = true;
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ACTIVATE => {
                app.dirty = true;
                app.pace(true);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CLOSE => {
                if app.close() {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_HOTKEY => {
                if let Some(action) = hotkeys::action(wp.0) {
                    app.hotkey(action);
                }
                LRESULT(0)
            }
            updates::WM_UPDATE_CHECKED => {
                app.update_checked();
                LRESULT(0)
            }
            updates::WM_UPDATE_READY => {
                app.update_ready();
                LRESULT(0)
            }
            tray::WM_QUIT_GOBY => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            tray::WM_TRAY => {
                match tray::event(lp) {
                    tray::SELECT | tray::KEY_SELECT => app.show(),
                    tray::MIDDLE_CLICK => app.hotkey(Hotkey::Toggle),
                    WM_CONTEXTMENU => {
                        let (items, acts): (Vec<_>, Vec<_>) = app.tray_menu().into_iter().unzip();
                        let chosen = tray::menu(hwnd, tray::anchor(wp), &items).and_then(|i| acts.get(i).copied());
                        let Some(app) = app_of(hwnd) else { return LRESULT(0) };
                        if chosen.is_some_and(|act| app.tray_act(act)) {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            m if m == taskbar_created() => {
                // Explorer restarted and forgot every icon.
                app.add_tray_icon();
                LRESULT(0)
            }
            WM_DESTROY => {
                tray::remove(hwnd);
                hotkeys::unregister(hwnd);
                app.remember_window();
                app.settings.save();
                let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Win;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(p));
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn window_size(w: f32, h: f32, dpi: u32, style: WINDOW_STYLE) -> (i32, i32) {
    let mut r = RECT {
        left: 0,
        top: 0,
        right: (w * dpi as f32 / 96.0).round() as i32,
        bottom: (h * dpi as f32 / 96.0).round() as i32,
    };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WS_EX_NOREDIRECTIONBITMAP, dpi);
    }
    (r.right - r.left, r.bottom - r.top)
}

/// Opens the window, or with `hidden` starts with only the tray icon.
pub fn run(hidden: bool) -> Result<()> {
    unsafe {
        // A second Goby would open a second engine into the same cable: bring the first one
        // forward instead.
        let _guard = CreateMutexW(None, true, w!("Goby.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // A start at sign-in finds Goby already up and leaves it where it is.
            if hidden {
                return Ok(());
            }
            if let Ok(hwnd) = FindWindowW(w!("GobyWindow"), PCWSTR::null()) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return Ok(());
        }
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
        let style = WS_OVERLAPPEDWINDOW;
        let (w, h) = window_size(WIDTH, HEIGHT, GetDpiForSystem(), style);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
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

        // Where it was closed, if that place is still on a monitor; else fit the monitor it
        // landed on: its DPI, and its work area on small laptops.
        let settings = Settings::load();
        let saved = settings.window.filter(|p| placement::restore(hwnd, p));
        let dpi = GetDpiForWindow(hwnd);
        let (mut w, mut h) = window_size(WIDTH, HEIGHT, dpi, style);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            let work = info.rcWork;
            let (min_w, min_h) = window_size(MIN_W, MIN_H, dpi, style);
            if let Some(p) = saved {
                // Its own size, unless that is smaller than the layout can be drawn in.
                if p.w < min_w || p.h < min_h {
                    let _ = SetWindowPos(hwnd, None, 0, 0, p.w.max(min_w), p.h.max(min_h), SWP_NOZORDER | SWP_NOMOVE);
                }
            } else {
                w = w.min(work.right - work.left).max(min_w);
                h = h.min(work.bottom - work.top).max(min_h);
                let x = work.left + (work.right - work.left - w).max(0) / 2;
                let y = work.top + (work.bottom - work.top - h).max(0) / 2;
                let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER);
            }
        }

        let host = Desktop::new(hwnd);
        let mut app = Box::new(App::new(host, settings, system_theme(), dpi as f32));
        app.host.start_maximized = saved.is_some_and(|p| p.maximized);
        app.apply_frame();
        app.add_tray_icon();
        app.use_hotkeys();
        app.host.check_for_update(&app.settings, Duration::from_secs(15));
        let hidden = hidden && app.host.tray_icon;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        if let Some(app) = app_of(hwnd) {
            app.pace(!hidden);
            if !hidden {
                let cmd = app.show_command();
                let _ = ShowWindow(hwnd, cmd);
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
