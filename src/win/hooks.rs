//! The global mouse hook and the foreground-window watcher that turns it off for excluded programs.

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PWSTR, Result};

use crate::config::{EXCLUDED_PROCESSES, EXCLUDED_TITLE_WORDS};
use crate::tracker::{Event, Point};

pub fn install_mouse_hook() -> Result<HHOOK> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(module.into()), 0)
    }
}

pub fn remove_mouse_hook(hook: HHOOK) {
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

/// Keep this fast: Windows silently removes low-level hooks that take too long.
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 {
            let p = Point {
                x: info.pt.x,
                y: info.pt.y,
            };
            let event = match wparam.0 as u32 {
                WM_RBUTTONDOWN => Some(Event::RightDown(p)),
                WM_MOUSEMOVE => Some(Event::Move(p)),
                WM_RBUTTONUP => Some(Event::RightUp(p)),
                _ => None,
            };
            if let Some(event) = event
                && super::app::on_mouse_event(event)
            {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

pub fn watch_foreground() {
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
}

unsafe extern "system" fn foreground_proc(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    #[cfg(debug_assertions)]
    eprintln!(
        "foreground: {:?} {:?} excluded={}",
        process_name(hwnd),
        window_title(hwnd),
        is_excluded(hwnd)
    );
    super::app::on_foreground_changed(hwnd);
}

pub fn is_excluded(hwnd: HWND) -> bool {
    if let Some(name) = process_name(hwnd)
        && EXCLUDED_PROCESSES
            .iter()
            .any(|e| e.eq_ignore_ascii_case(&name))
    {
        return true;
    }
    let title = window_title(hwnd).to_lowercase();
    EXCLUDED_TITLE_WORDS.iter().any(|w| title.contains(&w.to_lowercase()))
}

fn process_name(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(str::to_owned)
    }
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}
