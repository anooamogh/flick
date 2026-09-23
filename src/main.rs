// Release builds run as a tray app with no console window; debug builds keep the console for eprintln!.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod config;
mod direction;
mod tracker;
mod win;

fn main() -> windows::core::Result<()> {
    win::app::run()
}
