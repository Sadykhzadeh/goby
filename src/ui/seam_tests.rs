//! The window driven through its seams: the sample host in, a canvas that records what is
//! drawn out.

use std::cell::RefCell;

use super::*;
use crate::demo::{self, DemoHost};
use crate::draw::Color;

/// Keeps the text drawn and where; measures every character as 7 DIPs wide.
#[derive(Default)]
struct Record {
    texts: RefCell<Vec<(String, R)>>,
}

impl Canvas for Record {
    fn dpi(&self) -> f32 {
        96.0
    }
    fn fill(&self, _: R, _: f32, _: Color) {}
    fn stroke(&self, _: R, _: f32, _: Color, _: f32) {}
    fn circle(&self, _: f32, _: f32, _: f32, _: Color) {}
    fn line(&self, _: &[(f32, f32)], _: Color, _: f32) {}
    fn text(&self, s: &str, _: Font, r: R, _: Align, _: Color) {
        self.texts.borrow_mut().push((s.to_string(), r));
    }
    fn icon(&self, _: u16, _: Font, _: R, _: Color) {}
    fn measure(&self, s: &str, _: Font) -> f32 {
        s.chars().count() as f32 * 7.0
    }
    fn picture(&self, _: &str, _: u32, _: &mut dyn FnMut() -> Option<Vec<u8>>, _: R) -> bool {
        false
    }
    fn clip(&self, _: R) {}
    fn unclip(&self) {}
    fn shift(&self, _: f32, _: f32) {}
    fn unshift(&self) {}
}

fn window(settings: Settings, ru: bool, w: f32) -> (App<DemoHost>, Record) {
    let mut app = App::new(DemoHost::default(), settings, Theme::new(true, [0; 7]), ru, 96.0);
    app.resize(w, MIN_H);
    let g = Record::default();
    app.draw(&g);
    (app, g)
}

fn hit(app: &App<DemoHost>, h: Hit) -> R {
    app.hits.iter().find(|(_, x)| *x == h).map(|(r, _)| *r).expect("drawn")
}

#[test]
fn the_header_text_ends_before_the_save_button() {
    for ru in [false, true] {
        for w in [MIN_W, 900.0, WIDTH] {
            // Tuned, so the button shows, and with nobody listening, the longest line.
            let s = Settings { volume: 1.5, routed: Vec::new(), ..demo::settings() };
            let (app, g) = window(s, ru, w);
            let button = hit(&app, Hit::SavePreset);
            let card = app.lay.scope;
            for (text, r) in g.texts.borrow().iter() {
                let beside = r.t < button.b && r.b > button.t && r.l < button.l && r.t < card.t + 64.0;
                assert!(!beside || r.r <= button.l - 8.0, "{w} ru={ru}: {text:?} runs under the button");
            }
        }
    }
}

#[test]
fn the_save_button_has_room_either_side() {
    let (app, g) = window(Settings { volume: 1.5, ..demo::settings() }, false, MIN_W);
    let button = hit(&app, Hit::SavePreset);
    let texts = g.texts.borrow();
    let (_, label) = texts.iter().find(|(t, _)| t == "Save as my voice").unwrap();
    assert!(label.l >= button.l + 12.0 + 12.0 + 8.0, "icon and gap before the text");
    assert!(label.l + 16.0 * 7.0 <= button.r - 12.0, "room after the text");
}

#[test]
fn a_click_on_a_voice_reaches_the_host_and_is_saved() {
    let (mut app, _) = window(demo::settings(), false, WIDTH);
    app.down((app.lay.tabs.l + 10.0, app.lay.tabs.t + 10.0), false); // All
    app.draw(&Record::default());
    let saves = app.host.saves.get();
    let robot = voices::index_of("robot").unwrap();
    let tile = hit(&app, Hit::Tile(robot));
    assert!(app.down((tile.l + 5.0, tile.t + 5.0), false).is_none());
    app.up();
    assert_eq!(app.host.params.get().voice, robot);
    assert!(app.host.saves.get() > saves);
}

#[test]
fn hearing_yourself_sends_the_voice_to_a_real_output() {
    let (mut app, _) = window(demo::settings(), false, WIDTH);
    assert!(!app.host.live, "the cable alone is not heard");
    app.flip(Switch::Monitor);
    assert!(app.host.live && app.is_on(Switch::Monitor));
    app.flip(Switch::Monitor);
    assert!(!app.host.live);
}

#[test]
fn a_device_menu_chooses_by_key() {
    let (mut app, _) = window(demo::settings(), false, WIDTH);
    app.down((app.lay.nav_settings.l + 5.0, app.lay.nav_settings.t + 5.0), false);
    app.draw(&Record::default());
    let card = hit(&app, Hit::Pick(Pick::Out));
    let menu = app.down((card.l + 5.0, card.t + 5.0), false).expect("a menu");
    assert_eq!(menu.checked, Some(0));
    let speakers = menu.keys.iter().position(|k| k == "speakers").unwrap();
    app.menu_done(menu.pick, Some(menu.keys[speakers].clone()));
    assert_eq!(app.settings.output, "speakers");
    assert!(app.host.live, "speakers are heard");
}

#[test]
fn the_scope_redraws_only_when_the_tone_would_change() {
    let (mut app, _) = window(demo::settings(), false, WIDTH);
    app.shown = true;
    app.tick();
    assert!(app.voiced.iter().any(|s| s.abs() > 0.01), "the test tone is drawn");
    app.dirty = false;
    app.tick();
    app.tick();
    app.tick();
    assert!(!app.dirty, "nothing moves, nothing redraws");
}
