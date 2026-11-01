//! Russian or English, following the Windows display language.

use windows::Win32::Globalization::GetUserDefaultUILanguage;

pub fn russian() -> bool {
    let language = unsafe { GetUserDefaultUILanguage() };
    language & 0x3ff == 0x19
}

/// Picks the string for the current language.
pub fn pick(ru: &'static str, en: &'static str) -> &'static str {
    if russian() { ru } else { en }
}
