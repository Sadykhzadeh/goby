#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dsp;
mod engine;
mod fx;
mod ring;
mod settings;
mod ui;
mod voices;

fn main() -> windows::core::Result<()> {
    ui::run()
}
