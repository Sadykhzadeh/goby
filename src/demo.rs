//! A system with sample microphones, outputs and programs, for the website's copy of the
//! window and for tests. Settings stay in memory. With no live microphone the scope shows
//! the chosen voice played over a test tone; the website feeds it a real one when asked.

use std::cell::Cell;
use std::time::Duration;

use crate::dsp::Voice;
use crate::host::{AppState, Endpoint, Host, Link, MicApp, Pinned, SCOPE_LEN};
use crate::settings::Settings;
use crate::voices;

const CABLE_PLAY: &str = "cable-play";
/// What the app measures from a USB microphone to the cable, on average.
const LATENCY_MS: f32 = 28.0;

/// The voice and microphone settings as the window last set them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    pub voice: usize,
    pub tone: f32,
    pub volume: f32,
    pub gain_db: f32,
    /// NaN with the gate off.
    pub gate_db: f32,
    pub dry_mix: f32,
    pub enabled: bool,
}

pub struct DemoHost {
    /// Set by whoever drives the window, since there is no clock of its own on the web.
    pub clock: Duration,
    pub animations: bool,
    /// The browser's language, `navigator.language`.
    pub language: String,
    pub params: Cell<Params>,
    /// The voice should be heard: the window sends it to a real output, not just the cable.
    pub live: bool,
    /// Samples from a real microphone, before and after the voice, and whether they are new.
    pub heard: Box<[f32; 2 * SCOPE_LEN]>,
    pub fresh: bool,
    pub latency: f32,
    /// The last place a click asked to open.
    pub opened: Option<Link>,
    pub hotkeys: bool,
    pub saves: Cell<u32>,
    autostart: bool,
    shown: Option<Params>,
}

impl Default for DemoHost {
    fn default() -> Self {
        DemoHost {
            clock: Duration::ZERO,
            animations: true,
            language: "en".into(),
            params: Cell::new(Params::default()),
            live: false,
            heard: Box::new([0.0; 2 * SCOPE_LEN]),
            fresh: false,
            latency: LATENCY_MS,
            opened: None,
            hotkeys: true,
            saves: Cell::new(0),
            autostart: false,
            shown: None,
        }
    }
}

fn ep(id: &str, name: &str) -> Endpoint {
    Endpoint { id: id.into(), name: name.into() }
}

/// Settings of someone who has used Goby for a while: two programs hear it, a few voices
/// are starred.
pub fn settings() -> Settings {
    let list = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
    Settings {
        routed: list(&["cs2.exe", "discord.exe"]),
        favorites: list(&["minion", "robot", "helium", "demon"]),
        gate: true,
        cable_named_tried: true,
        tray_told: true,
        update_checked: u64::MAX,
        ..Settings::default()
    }
}

/// The last `SCOPE_LEN` samples of a buzzy 140 Hz tone, plain and through the voice.
fn tone_through(p: Params) -> ([f32; SCOPE_LEN], [f32; SCOPE_LEN]) {
    const SR: u32 = 48_000;
    let tone: Vec<f32> = (0..SR as usize / 8)
        .map(|i| {
            let t = i as f32 / SR as f32;
            (1..6).map(|h| (t * 140.0 * h as f32 * std::f32::consts::TAU).sin() / h as f32).sum::<f32>() * 0.12
        })
        .collect();
    let mut out = tone.clone();
    let mut voice = Voice::new(SR);
    let (def, tone_st) = if p.enabled { (&voices::ALL[p.voice], p.tone) } else { (&voices::ALL[0], 0.0) };
    voice.set(&def.recipe(), tone_st, p.volume);
    voice.process(&mut out);
    let mix = if p.enabled { p.dry_mix } else { 0.0 };
    let tail = tone.len() - SCOPE_LEN;
    let raw = std::array::from_fn(|i| tone[tail + i]);
    let voiced = std::array::from_fn(|i| out[tail + i] * (1.0 - mix) + tone[tail + i] * mix);
    (raw, voiced)
}

impl Host for DemoHost {
    fn now(&self) -> Duration {
        self.clock
    }

    fn animations(&self) -> bool {
        self.animations
    }

    fn language(&self) -> String {
        self.language.clone()
    }

    fn save(&self, _: &Settings) {
        self.saves.set(self.saves.get() + 1);
    }

    fn devices(&mut self, capture: bool) -> Vec<Endpoint> {
        if capture {
            vec![ep("mic", "Microphone"), ep("headset", "Headset Microphone"), ep("webcam", "Webcam Microphone")]
        } else {
            vec![
                ep(CABLE_PLAY, "Goby (VB-Audio Virtual Cable)"),
                ep("speakers", "Speakers"),
                ep("headphones", "Headphones"),
            ]
        }
    }

    fn default_device(&mut self, capture: bool) -> Option<Endpoint> {
        Some(if capture { ep("mic", "Microphone") } else { ep("headphones", "Headphones") })
    }

    fn cable(&mut self) -> Option<(Endpoint, Endpoint)> {
        Some((ep(CABLE_PLAY, "Goby (VB-Audio Virtual Cable)"), ep("cable-record", "Goby Mic (VB-Audio Virtual Cable)")))
    }

    fn cable_installed(&mut self) -> bool {
        true
    }

    fn name_cable(&mut self, _: &Endpoint, _: &Endpoint) {}

    fn start(&mut self, _: &str, _: bool, outputs: &[String]) -> Result<(), String> {
        self.live = outputs.iter().any(|o| o != CABLE_PLAY);
        if !self.live {
            self.latency = LATENCY_MS;
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.live = false;
    }

    fn running(&self) -> bool {
        true
    }

    fn failed(&self) -> bool {
        false
    }

    fn exclusive(&self) -> bool {
        false
    }

    fn voice(&self, voice: usize, tone: f32, volume: f32) {
        self.params.set(Params { voice, tone, volume, ..self.params.get() });
    }

    fn mic(&self, gain_db: f32, gate_db: Option<f32>, dry_mix: f32, enabled: bool) {
        let gate_db = gate_db.unwrap_or(f32::NAN);
        self.params.set(Params { gain_db, gate_db, dry_mix, enabled, ..self.params.get() });
    }

    fn latency_ms(&self) -> f32 {
        self.latency
    }

    fn scope(&mut self, raw: &mut [f32; SCOPE_LEN], voiced: &mut [f32; SCOPE_LEN]) -> bool {
        if self.live {
            if !std::mem::take(&mut self.fresh) {
                return false;
            }
            raw.copy_from_slice(&self.heard[..SCOPE_LEN]);
            voiced.copy_from_slice(&self.heard[SCOPE_LEN..]);
            self.shown = None;
            return true;
        }
        // Only what changes the tone's sound redraws it.
        let mut p = self.params.get();
        (p.gain_db, p.gate_db) = (0.0, 0.0);
        if self.shown == Some(p) {
            return false;
        }
        self.shown = Some(p);
        (*raw, *voiced) = tone_through(p);
        true
    }

    fn recent_apps(&mut self) -> Vec<MicApp> {
        let app = |exe: &str, active: bool| MicApp {
            exe: exe.into(),
            name: crate::host::display_name(exe),
            path: String::new(),
            last_used: 0,
            active,
        };
        vec![
            app("telegram.exe", true),
            app("cs2.exe", true),
            app("discord.exe", false),
            app("obs64.exe", false),
            app("chrome.exe", false),
            app("steam.exe", false),
        ]
    }

    fn pin(
        &mut self,
        _: &str,
        routed: &[String],
        unroute: &mut Vec<String>,
        _: &mut Vec<(String, String)>,
        watching: bool,
    ) -> Pinned {
        unroute.clear();
        let running = |exe: &str| matches!(exe, "cs2.exe" | "discord.exe" | "telegram.exe");
        let states = routed.iter().map(|e| if running(e) { AppState::OnGoby } else { AppState::Waiting }).collect();
        // CS2 is in a match; Telegram picked a microphone of its own.
        let live = watching.then(|| {
            let live = routed.iter().map(|e| e == "cs2.exe").collect();
            let elsewhere = routed.iter().map(|e| (e == "telegram.exe").then(|| "Microphone".to_string())).collect();
            (live, elsewhere)
        });
        Pinned { states, live }
    }

    fn repin(&mut self) {}

    fn icon(&self, _: &str, _: u32) -> Option<Vec<u8>> {
        None
    }

    fn open(&mut self, link: Link) {
        self.opened = Some(link);
    }

    fn capture(&mut self, _: bool) {}

    fn autostart(&self) -> bool {
        self.autostart
    }

    fn set_autostart(&mut self, on: bool) {
        self.autostart = on;
    }

    fn hotkeys(&mut self, on: bool) -> bool {
        self.hotkeys = on;
        true
    }

    fn check_for_update(&mut self, _: &Settings, _: Duration) {}

    fn update(&mut self) {}

    fn installed(&self) -> bool {
        false
    }
}
