#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dsp;
mod engine;
mod fx;
mod gfx;
mod mic;
mod ring;
mod settings;
mod theme;
mod ui;
mod voices;

fn main() -> windows::core::Result<()> {
    ui::run()
}
