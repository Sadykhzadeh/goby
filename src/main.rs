#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps;
mod dsp;
mod engine;
mod fx;
mod gfx;
mod mic;
mod policy;
mod ring;
mod settings;
mod theme;
mod ui;
mod voices;

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // Relaunched elevated by the window to rename the virtual cable; does that and exits.
    if let [_, flag, render, capture] = args.as_slice()
        && flag == "--name-cable"
    {
        return policy::name_cable(render, capture);
    }
    ui::run()
}
