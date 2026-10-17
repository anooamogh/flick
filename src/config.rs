//! Hardcoded settings (v1 has no settings file).

use crate::direction::Direction;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_A, VK_C, VK_CONTROL, VK_LEFT, VK_MENU, VK_RIGHT, VK_V, VK_X, VK_Y, VK_Z,
};

/// Tray icon, rendered from this emoji at startup.
pub const TRAY_EMOJI: &str = "🪄";

/// Hold this long without releasing and the menu appears.
pub const MENU_DELAY_MS: u32 = 250;

/// Movement (physical pixels) needed before a flick counts as a direction.
pub const DEAD_ZONE_PX: i32 = 30;

/// While one of these programs is the foreground window, the mouse hook is removed entirely.
/// Matched case-insensitively against the executable's file name.
pub const EXCLUDED_PROCESSES: &[&str] = &[
    "League of Legends.exe", // the game, not LeagueClient.exe / LeagueClientUx.exe
    "EtG.exe",               // Enter the Gungeon
];

pub struct Action {
    pub label: &'static str,
    pub modifier: VIRTUAL_KEY,
    pub key: VIRTUAL_KEY,
}

pub fn action(dir: Direction) -> Action {
    let (label, modifier, key) = match dir {
        Direction::Up => ("Select All", VK_CONTROL, VK_A),
        Direction::UpRight => ("Redo", VK_CONTROL, VK_Y),
        Direction::Right => ("Next", VK_MENU, VK_RIGHT), // browser forward
        Direction::DownRight => ("Copy", VK_CONTROL, VK_C),
        Direction::Down => ("Paste", VK_CONTROL, VK_V),
        Direction::DownLeft => ("Cut", VK_CONTROL, VK_X),
        Direction::Left => ("Prev", VK_MENU, VK_LEFT), // browser back
        Direction::UpLeft => ("Undo", VK_CONTROL, VK_Z),
    };
    Action {
        label,
        modifier,
        key,
    }
}
