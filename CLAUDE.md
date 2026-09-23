# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

**Flick**: a Windows-only, desktop-wide marking menu on the right mouse button, written in Rust with the `windows` crate. Tray app with no main window.

- Quick right-click → normal context menu (the swallowed click is replayed).
- Hold and flick in one of 8 directions, release → action fires (keyboard shortcut sent to the foreground window).
- Hold ~250ms → ring menu appears at the press point; releasing in the center cancels.
- Hook is fully removed while an excluded program (League of Legends game) is the foreground window.

## Commands

```
cargo build                 # debug build; keeps a console so eprintln! output is visible
cargo build --release       # tray-only exe (windows_subsystem = "windows") at target/release/flick.exe
cargo run                   # run it (only one instance allowed; exit via the tray icon menu)
cargo test                  # unit tests for direction, tracker and anim logic
cargo test tracker::tests::quick_click_replays_click   # single test
cargo clippy --all-targets
cargo fmt
```

Real mouse behaviour can't be unit-tested; verify hook/overlay/input changes by running the app and trying it by hand.

## Architecture

- `src/direction.rs`, `src/tracker.rs`: **pure logic, no Windows calls**, fully unit-tested. `Tracker::handle(Event) -> Response { swallow, effect }` is the whole gesture state machine (click vs flick vs cancel, when to show/highlight the menu). Change behaviour here first, with tests.
- `src/anim.rs`: pure, unit-tested visual state of the ring (bubble angle gliding the short way round, bubble glow, open/close fade), eased with time-based exponential smoothing so it's frame-rate independent.
- `src/config.rs`: hardcoded settings: menu delay, dead zone, excluded process names / window titles, and the direction → label + key chord table.
- `src/win/`: **all `unsafe` / Win32 code lives here.**
  - `app.rs`: startup (single-instance mutex, per-monitor DPI v2), hidden control window, message loop, and effect execution.
  - `hooks.rs`: `WH_MOUSE_LL` hook and the `EVENT_SYSTEM_FOREGROUND` watcher that installs/removes it based on exclusions.
  - `overlay.rs`: layered, click-through, no-activate topmost window drawn with Direct2D/DirectWrite into a DIB, pushed with `UpdateLayeredWindow`.
  - `input.rs`: `SendInput` for replayed right-clicks and key chords.
  - `tray.rs`: tray icon, menu (Start with Windows / Exit), HKCU `Run` registry entry.

Everything runs on one thread. Key invariants:
- The mouse hook callback must stay fast (Windows silently drops slow low-level hooks). It only calls `Tracker::handle`, queues the effect, and posts `WM_EFFECTS`; drawing and `SendInput` happen later in the message loop.
- Injected events (`LLMHF_INJECTED`) are always passed through. That's how replayed clicks avoid being re-captured.
- Menu delay is a `SetTimer` on the control window feeding `Event::MenuDelayElapsed`; the tracker has no clock.
- Animation frames come from a second timer (`TIMER_FRAME`, with `timeBeginPeriod(1)`) that runs only while the overlay is animating and stops once `Overlay::tick` reports it has settled or finished fading out. Overlay resources (brushes, gradient) are cached per DPI scale in `Surface`, not created per frame.
- Coordinates are physical pixels (process is per-monitor DPI aware); the overlay scales its drawing by the monitor's DPI.

## Behavioral guidelines

These bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think before coding

Don't assume. Don't hide confusion. Surface tradeoffs.

- State assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them — don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop, name what's confusing, and ask.

### 2. Simplicity first

Minimum code that solves the problem. Nothing speculative.

- No features beyond what was asked.
- No abstractions for single-use code.
- No flexibility or configurability that wasn't requested.
- No error handling for impossible scenarios.
- If 200 lines could be 50, rewrite it. Would a senior engineer call it overcomplicated? If yes, simplify.

### 3. Surgical changes

Touch only what you must. Clean up only your own mess.

- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- Mention unrelated dead code; don't delete it.
- Remove imports/variables/functions that *your* changes made unused, but not pre-existing dead code unless asked.

Every changed line should trace directly to the user's request.

### 4. Goal-driven execution

Define success criteria. Loop until verified.

- "Add validation" → write tests for invalid inputs, then make them pass.
- "Fix the bug" → write a test that reproduces it, then make it pass.
- "Refactor X" → ensure tests pass before and after.

For multi-step tasks, state a brief plan:

```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria allow independent iteration; weak ones ("make it work") require constant clarification.
