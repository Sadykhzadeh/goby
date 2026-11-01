#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dsp;
mod engine;
mod ring;
mod settings;
mod ui;

fn main() -> windows::core::Result<()> {
    ui::run()
}
