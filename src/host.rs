//! What the window needs from the system it runs on, and the plain data it gets back.
//! The app answers over WASAPI, the registry and Win32 (`ui::win`); the website and the
//! tests answer with sample devices and programs (`demo`).

use std::time::Duration;

use crate::settings::Settings;

/// Samples kept for the scope, before and after the voice.
pub const SCOPE_LEN: usize = 2048;

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MicApp {
    /// Lower-case executable name, e.g. "cs2.exe": stable across updates that move the folder.
    pub exe: String,
    /// What to show: the executable name without ".exe".
    pub name: String,
    /// Where the executable lives, for its icon.
    pub path: String,
    /// FILETIME of the last time it opened the mic.
    pub last_used: u64,
    /// It has the mic open right now.
    pub active: bool,
}

/// Where a program chosen to hear Goby stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AppState {
    /// Running and recording from Goby.
    OnGoby,
    /// Not running; it will be pinned when it starts.
    Waiting,
    /// Running, but Windows will only pin it once it has opened audio.
    Pending,
}

/// What pinning the chosen programs found, in `settings.routed` order.
#[derive(Clone, Debug, PartialEq)]
pub struct Pinned {
    pub states: Vec<AppState>,
    /// Asked only while the Programs page is watched: whether each records from Goby right
    /// now, and the real microphone it records from instead, when it picked its own.
    pub live: Option<(Vec<bool>, Vec<Option<String>>)>,
}

/// Places outside the window that a click opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    /// The author's site.
    Author,
    /// The VB-Cable download page.
    Cable,
    /// The Downloads folder, where the cable's archive lands.
    Downloads,
}

/// What the global hotkeys ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hotkey {
    Toggle,
    Next,
    Previous,
}

/// The keys the window answers to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Escape,
    Back,
}

pub trait Host {
    /// Time since a fixed moment; only differences matter.
    fn now(&self) -> Duration;
    /// Pages may animate (Windows' "Animation effects", or no reduced-motion request).
    fn animations(&self) -> bool;
    /// The language the system shows itself in, as a tag such as "ru-RU".
    fn language(&self) -> String;
    fn save(&self, settings: &Settings);

    /// Active microphones (`capture`) or outputs, cable included.
    fn devices(&mut self, capture: bool) -> Vec<Endpoint>;
    fn default_device(&mut self, capture: bool) -> Option<Endpoint>;
    /// The virtual cable, (play side, record side), if installed.
    fn cable(&mut self) -> Option<(Endpoint, Endpoint)>;
    /// Gives the cable Goby's names, behind one elevation prompt.
    fn name_cable(&mut self, play: &Endpoint, record: &Endpoint);

    /// Runs the voice from `mic` to `outputs` (the first is the cable or chosen output).
    fn start(&mut self, mic: &str, exclusive: bool, outputs: &[String]) -> Result<(), String>;
    /// Stops it and forgets an earlier failure.
    fn stop(&mut self);
    fn running(&self) -> bool;
    /// A device went away under the running voice.
    fn failed(&self) -> bool;
    /// The microphone really runs in exclusive mode.
    fn exclusive(&self) -> bool;
    fn voice(&self, voice: usize, tone: f32, volume: f32);
    fn mic(&self, gain_db: f32, gate_db: Option<f32>, dry_mix: f32, enabled: bool);
    fn latency_ms(&self) -> f32;
    /// The latest samples before and after the voice, oldest first; false when nothing
    /// changed since the last call.
    fn scope(&mut self, raw: &mut [f32; SCOPE_LEN], voiced: &mut [f32; SCOPE_LEN]) -> bool;

    /// Programs that recently used the microphone, most recent first.
    fn recent_apps(&mut self) -> Vec<MicApp>;
    /// Points the chosen programs at the cable's record side `record`, hands the ones in
    /// `unroute` back (dropping those done), and adds the paths of programs it found to
    /// `paths` (lower-case exe, full path).
    fn pin(
        &mut self,
        record: &str,
        routed: &[String],
        unroute: &mut Vec<String>,
        paths: &mut Vec<(String, String)>,
        watching: bool,
    ) -> Pinned;
    /// Forgets which running programs are already pinned, so all are asked again.
    fn repin(&mut self);
    /// A program's icon as premultiplied BGRA, `px` square.
    fn icon(&self, path: &str, px: u32) -> Option<Vec<u8>>;

    fn open(&mut self, link: Link);
    /// Keeps the pointer while a slider or scrollbar is dragged.
    fn capture(&mut self, on: bool);
    fn autostart(&self) -> bool;
    fn set_autostart(&mut self, on: bool);
    /// Turns the global hotkeys on or off; false when another program holds one.
    fn hotkeys(&mut self, on: bool) -> bool;
    /// Looks for a newer release after `delay`, if `settings` says it is time.
    fn check_for_update(&mut self, settings: &Settings, delay: Duration);
    /// Installs the newer release, or opens its page for a copy that is not installed.
    fn update(&mut self);
    /// This copy is the installed one, which updates itself.
    fn installed(&self) -> bool;
}

/// What people call a program: the product name for well-known ones, whose executables
/// are often cryptic (`cs2.exe`, `obs64.exe`), otherwise the file name with a capital.
pub fn display_name(exe: &str) -> String {
    let known = match exe {
        "cs2.exe" => "Counter-Strike 2",
        "dota2.exe" => "Dota 2",
        "r5apex.exe" | "r5apex_dx12.exe" => "Apex Legends",
        "valorant-win64-shipping.exe" => "Valorant",
        "fortniteclient-win64-shipping.exe" => "Fortnite",
        "robloxplayerbeta.exe" => "Roblox",
        "javaw.exe" => "Minecraft",
        "discord.exe" => "Discord",
        "telegram.exe" => "Telegram",
        "teams.exe" | "ms-teams.exe" => "Microsoft Teams",
        "zoom.exe" => "Zoom",
        "skype.exe" => "Skype",
        "whatsapp.exe" => "WhatsApp",
        "obs64.exe" | "obs32.exe" => "OBS Studio",
        "chrome.exe" => "Google Chrome",
        "msedge.exe" => "Microsoft Edge",
        "firefox.exe" => "Firefox",
        "steam.exe" | "steamwebhelper.exe" => "Steam",
        "nvidia broadcast.exe" => "NVIDIA Broadcast",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    // ASCII only: full Unicode case mapping would add 17 KB of tables for one letter.
    let mut name = exe.strip_suffix(".exe").unwrap_or(exe).to_string();
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    name
}
