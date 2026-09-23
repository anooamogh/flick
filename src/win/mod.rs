//! Everything that calls into Windows lives here. The logic in `tracker` and `direction` does not.

pub mod app;
mod hooks;
mod icon;
mod input;
mod overlay;
mod tray;
