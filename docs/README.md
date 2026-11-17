# Wintil Technical Guide & Internals

Wintil is a native Rust Windows utility for global window hotkeys, always-on-top pinning, tray window hiding, and event-driven script automations. It was rewritten from AutoHotkey (`win-utils`) using direct Win32 API calls to eliminate hook input lag and stuck-modifier bugs.

**Related Docs:**
- [Automations Manager](Automations%20Manager.md) — `automation.toml` schema, triggers, subservices, and portable bundling
- [Changelog](CHANGELOG.md) — Release history

---

## 1. Why Rust Over AutoHotkey?

1. **Zero Input Latency**: AHK uses `WH_KEYBOARD_LL`, routing every physical keystroke through a userland hook. Wintil uses Win32 `RegisterHotKey`, so the Windows kernel dispatches shortcuts directly with 0 ms overhead.
2. **Clean Modifier State**: When hiding or closing the focused window mid-chord (`Alt + Shift + H`, `Alt + Q`), Windows can drop modifier key-up events. Wintil inspects `GetAsyncKeyState` and cleanly releases only held modifiers without injecting phantom keystrokes.

---

## 2. Core Features & Architecture

### Window Hiding (`Window Hider`)
- **Manual Hiding (`Alt + Shift + H`)**: Calls `ShowWindow(hwnd, SW_HIDE)` so the taskbar removes the button while the process stays running. Restore with `Alt + Shift + U` (last), `Alt + Shift + R` (all), or from **Options → Window Ops**.
- **Auto-Hide Service**: Lazy-loaded background worker polling at `service_interval_ms` (default `750 ms`) for executables listed in `hider.auto_hide_executables`. Unloads automatically when idle/empty.

### Crash Recovery Sentry (`Crash Sentry`)
- Spawns a detached watchdog process (`wintil.exe --sentry <PID>`) waiting on the main process handle (`WaitForSingleObject`).
- Tracks pinned/hidden windows in an atomic state journal (`$HOME\.config\wintil\wintil_state.journal`).
- On unexpected exit or crash, the sentry restores all hidden windows, strips `[Pinned]` title tags, logs diagnostics to `$HOME\.config\wintil\logs\`, and can automatically respawn Wintil (protected by a 3-crash / 60s circuit breaker).

### Pinning Always-on-Top (`Window Pinner`)
- `Alt + P` toggles `HWND_TOPMOST` via `SetWindowPos`.
- Optional **Pin HUD** translucent floating badge (`show_pin_hud`), title bar tag (`pin_title_tag`), and **Border Stroke** outline (`pin_stroke.enabled`, `thickness`, `color`) whose `EVENT_OBJECT_LOCATIONCHANGE` hook only attaches while enabled and windows are pinned.

### Fake Fullscreen (`Alt + Shift + F`)
- **Usage**: Press `Alt + Shift + F` (configurable via `hotkeys.fake_fullscreen` in `wintil.toml` or **Options → Keybinds**) to toggle borderless work-area fullscreen on the active window. Pressing it again—or exiting Wintil—restores the window's original chrome, maximized state, and placement.
- **Why Fake Fullscreen instead of native Fullscreen (`F11`)?**
  - **No DWM Occlusion / Direct Flip**: Native fullscreen covers `rcMonitor` and triggers DWM fullscreen occlusion, causing black-screen flickers or delays when Alt-Tabbing or opening overlays/flyouts. Fake fullscreen sizes to `rcWork` (`height - 1px`), keeping the taskbar visible and transitions instant.
  - **Works on Any Window**: Brings borderless fullscreen to apps that have no built-in fullscreen shortcut, without hiding application-internal UI you still want to see.

### Quick Settings Flyout GUI
Left-clicking the tray icon opens a DPI-aware popout flyout docked near the taskbar with three tabs:
- **Services**: Live status for **Key Remapper**, **Crash Sentry**, **Automations Engine**, **Window Pinner**, and **Window Hider**, plus one-click **Subservices** (`Run Fix`). Crash Sentry highlights respawn status and escalates new errors since the last GUI session (`!` → `!` → `!!!` → `Critical Errors!`).
- **Options**:
  - **Keybinds**: Global shortcut legend and interactive runtime **Rebind** editor (suspends active hooks while recording, blocks mouse inputs and conflicting keybinds).
  - **Window Ops**: Scrollable stacked view of **Pinned Windows**, **Pin Border & Indicator Options**, **Hidden Windows** (`Restore Last` / `Restore All` / `Clear List`), and **Auto-Hide Service Rules** manager.
  - **Automations**: Scrollable list of event-driven scripts with an inline editor for name, description, subservice title, triggers, and File Explorer script cloning.
- **Settings**: Windows Startup Task & UAC elevation toggle, Crash Watchdog toggles & log controls (**Open Logs**, **Clear Logs**), **On-hover tooltips** (off by default, 900 ms dwell), **Keep GUI Open**, config hot-reload, modifier flush, and **Exit Wintil** confirmation.

---

## 3. Command-Line Reference

```powershell
.\wintil.exe --version                            # Print version
.\wintil.exe --help                               # Show CLI usage
.\wintil.exe --audit-keys                         # Live read-only modifier state monitor
.\wintil.exe --flush-keys                         # Force-release latched modifier keys
.\wintil.exe --list-automations                   # List loaded automations and triggers
.\wintil.exe --run-automation <name>              # Manually execute an automation by name
.\wintil.exe --sentry <PID>                       # Internal crash watchdog mode
```

---

## 4. Configuration & Logs Layout

Runtime files live in `$HOME\.config\wintil\`:
- `wintil.toml` — Main configuration (hot-reloaded automatically on save; falls back to `./wintil.toml` or `<exe_dir>\wintil.toml` if absent).
- `automations/` — User automation folders (`automation.toml` + script). See [Automations Manager](Automations%20Manager.md).
- `logs/` — `wintil.log` (master log), `wintil_events.jsonl` (structured telemetry), `wintil_crash.log`, `automations.log`, `keyboard_audit.log`.
- `wintil_state.journal` — Atomic crash-recovery window state journal.

---

## 5. Building from Source

```powershell
cargo test                    # Run unit tests
cargo build --release         # Build optimized binary (target\release\wintil.exe)
```

---

## 6. Helper Scripts (`scripts/`)

```powershell
python scripts\bump_version.py check                    # Verify version consistency across Cargo.toml/docs
python scripts\bump_version.py patch --build --test     # Bump version, test, and build
scripts\bump.bat patch                                  # Batch wrapper for bump_version.py
scripts\build.bat                                       # Stop task, build release, restart task
```
