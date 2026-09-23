//! Startup, the message loop, and the glue between the hook, the tracker, and the effects.
//!
//! Everything runs on one thread. The mouse hook only updates the tracker and queues effects;
//! the queued effects (drawing, SendInput) run later from the message loop so the hook returns fast.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use windows::Win32::Foundation::*;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Result, w};

use super::overlay::Overlay;
use super::{hooks, input, tray};
use crate::config::{self, DEAD_ZONE_PX, MENU_DELAY_MS};
use crate::tracker::{Effect, Event, Outcome, Tracker};

const WM_EFFECTS: u32 = WM_APP + 2;
const TIMER_MENU: usize = 1;
const TIMER_FRAME: usize = 2;
const FRAME_MS: u32 = 8;

thread_local! {
    static TRACKER: RefCell<Tracker> = RefCell::new(Tracker::new(DEAD_ZONE_PX));
    static EFFECTS: RefCell<VecDeque<Effect>> = const { RefCell::new(VecDeque::new()) };
    static MOUSE_HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static CONTROL: Cell<HWND> = Cell::new(HWND::default());
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static FRAMES_RUNNING: Cell<bool> = const { Cell::new(false) };
}

pub fn run() -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, false, w!("Local\\FlickGestureMenu"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(()); // already running
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = WNDCLASSW {
            lpfnWndProc: Some(control_proc),
            hInstance: instance,
            lpszClassName: w!("FlickControl"),
            ..Default::default()
        };
        RegisterClassW(&class);
        // Hidden window that owns the tray icon, timer, and effect messages.
        let control = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("FlickControl"),
            w!("Flick"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )?;
        CONTROL.set(control);
        let overlay = Overlay::new(instance)?;
        OVERLAY.set(Some(overlay));
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));

        tray::add(control);
        hooks::watch_foreground();
        on_foreground_changed(GetForegroundWindow());

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}

/// Called from the mouse hook. Returns true if the event should be swallowed.
pub fn on_mouse_event(event: Event) -> bool {
    let response = TRACKER.with_borrow_mut(|t| t.handle(event));
    if let Some(effect) = response.effect {
        EFFECTS.with_borrow_mut(|q| q.push_back(effect));
        unsafe {
            let _ = PostMessageW(Some(CONTROL.get()), WM_EFFECTS, WPARAM(0), LPARAM(0));
        }
    }
    response.swallow
}

pub fn on_foreground_changed(hwnd: HWND) {
    set_hook_enabled(!hooks::is_excluded(hwnd));
}

fn set_hook_enabled(enabled: bool) {
    match (enabled, MOUSE_HOOK.get()) {
        (true, None) => match hooks::install_mouse_hook() {
            Ok(hook) => MOUSE_HOOK.set(Some(hook)),
            Err(e) => eprintln!("failed to install mouse hook: {e}"),
        },
        (false, Some(hook)) => {
            hooks::remove_mouse_hook(hook);
            MOUSE_HOOK.set(None);
            TRACKER.with_borrow_mut(Tracker::reset);
            EFFECTS.with_borrow_mut(VecDeque::clear);
            stop_menu();
        }
        _ => {}
    }
}

fn run_effect(effect: Effect) {
    match effect {
        Effect::Pressed => unsafe {
            SetTimer(Some(CONTROL.get()), TIMER_MENU, MENU_DELAY_MS, None);
        },
        Effect::ShowMenu { origin, highlight } => animate(|o| o.show(origin, highlight)),
        Effect::Highlight(highlight) => animate(|o| {
            o.set_highlight(highlight);
            Ok(())
        }),
        Effect::Finish(outcome) => {
            stop_menu();
            match outcome {
                Outcome::Click => input::replay_right_click(),
                Outcome::Cancel => {}
                Outcome::Fire(dir) => input::send_chord(&config::action(dir)),
            }
        }
    }
}

fn stop_menu() {
    unsafe {
        let _ = KillTimer(Some(CONTROL.get()), TIMER_MENU);
    }
    animate(|o| {
        o.hide();
        Ok(())
    });
}

/// Applies a change to the overlay and makes sure frames run while it has something to animate.
fn animate(f: impl FnOnce(&mut Overlay) -> Result<()>) {
    let active = OVERLAY.with_borrow_mut(|o| {
        let o = o.as_mut()?;
        if let Err(e) = f(o) {
            eprintln!("overlay: {e}");
        }
        Some(o.is_active())
    });
    if active == Some(true) {
        set_frames_running(true);
    }
}

fn set_frames_running(running: bool) {
    if FRAMES_RUNNING.replace(running) == running {
        return;
    }
    unsafe {
        if running {
            // 1ms timer resolution while animating, so the frame timer isn't stuck at ~64fps.
            timeBeginPeriod(1);
            SetTimer(Some(CONTROL.get()), TIMER_FRAME, FRAME_MS, None);
        } else {
            let _ = KillTimer(Some(CONTROL.get()), TIMER_FRAME);
            timeEndPeriod(1);
        }
    }
}

unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_EFFECTS => {
            while let Some(effect) = EFFECTS.with_borrow_mut(VecDeque::pop_front) {
                run_effect(effect);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_MENU => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_MENU);
            }
            let response = TRACKER.with_borrow_mut(|t| t.handle(Event::MenuDelayElapsed));
            if let Some(effect) = response.effect {
                run_effect(effect);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_FRAME => {
            let running = OVERLAY.with_borrow_mut(|o| {
                o.as_mut().is_some_and(|o| {
                    o.tick().unwrap_or_else(|e| {
                        eprintln!("overlay: {e}");
                        false
                    })
                })
            });
            if !running {
                set_frames_running(false);
            }
            LRESULT(0)
        }
        tray::WM_TRAY => {
            let mouse_msg = (lparam.0 & 0xFFFF) as u32;
            if (mouse_msg == WM_RBUTTONUP || mouse_msg == WM_LBUTTONUP) && tray::show_menu(hwnd) {
                set_hook_enabled(false);
                tray::remove(hwnd);
                unsafe { PostQuitMessage(0) };
            }
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.get() => {
            // Explorer restarted; the tray icon has to be re-added.
            tray::add(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
