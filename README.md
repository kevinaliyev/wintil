# Wintil

Wintil (*When till Microsoft adds these*) is a small, open-source Windows desktop utility for keyboard-driven window management, pinning, tray hiding, and event-driven automations.

Originally started as a set of AutoHotkey scripts (`win-utils`), Wintil was rewritten in Rust to fix:
- **Clunky window management**: instant hotkeys to maximize/fullscreen, minimize, close, kill, pin always-on-top, or hide windows to the tray without intrusive `WH_KEYBOARD_LL` hook lag.
- **Unreliable Task Scheduler wake behavior**: a built-in event-driven script runner (originally built to fix an ASUS G14 USB keyboard disconnect on wake).
- **Crash recovery**: a separate lightweight **Crash Sentry** process that logs errors, optionally respawns Wintil on crash, and restores any hidden or pinned windows if Wintil terminates unexpectedly.

Wintil has a tiny footprint, is configurable via its tray GUI or `wintil.toml`, and can be run portably.

---

## Default Shortcuts

All shortcuts can be rebound live in **Options → Keybinds** or edited in `wintil.toml`:

| Shortcut | Action | Notes |
| :--- | :--- | :--- |
| `Alt + F` | Maximize / Restore | Toggles between maximized and restored bounds |
| `Alt + H` | Minimize | Minimizes the active window |
| `Alt + Q` | Close | Sends `WM_CLOSE` (same as `Alt + F4`) |
| `Alt + Shift + Q` | Kill Process | Terminates the active window's process |
| `Alt + Ctrl + Shift + Q` | Kill All by Name | Force-kills all processes matching the active executable |
| `Alt + Shift + H` | Hide to Tray | Hides the active window from desktop and taskbar |
| `Alt + P` | Toggle Pin | Pins/unpins always-on-top with an optional badge and border |
| `Alt + Shift + U` | Restore Last Hidden | Restores the most recently hidden window |
| `Alt + Shift + R` | Restore All Hidden | Restores all hidden windows back to the desktop |
| `Alt + Shift + Backspace` | Unstick Keys | Flushes latched modifier keys (`Ctrl`, `Alt`, `Shift`, `Win`) |

---

## Building from Source

Requires Rust and Cargo:

```powershell
cd wintil

# Run unit tests
cargo test

# Build the standalone release binary
cargo build --release

# Output binary:
.\target\release\wintil.exe
```

---

## Intended Usage & Configuration

- **Recommended (Elevated at Startup)**: Left-click the tray icon → **Settings**, then enable **Autostart** and **Elevated**. Running elevated lets Wintil manage elevated/admin windows and run privileged automations.
- **Standard User**: Works normally without elevation for standard desktop windows.
- **Portable Mode (USB)**: `wintil.exe` is a single self-contained binary. You can place a `wintil.toml` and an `automations\` folder directly next to `wintil.exe` to carry your keybinds and scripts on a USB drive.

### Config Spec (`wintil.toml`)

By default, Wintil reads and writes configuration in `$HOME\.config\wintil\wintil.toml` (falling back to `./wintil.toml` or `<exe_dir>\wintil.toml` if not present). Edits to `wintil.toml` hot-reload automatically without restarting.

```toml
[general]
single_instance = true
show_pin_hud = true
pin_title_tag = true
show_tooltips = false
notification_duration_ms = 800

[pin_hud]
text_pinned = "Pinned"
text_unpinned = "Unpinned"
bg_color = "#383E48"
text_color = "#FFFFFF"
stroke_color = "#000000"
opacity = 0.75

[pin_stroke]
enabled = false
thickness = 2
color = "burgundy"

[hotkeys]
maximize_toggle = "Alt+F"
minimize_active = "Alt+H"
close_active    = "Alt+Q"
kill_active     = "Alt+Shift+Q"
kill_all_by_exe = "Alt+Ctrl+Shift+Q"
hide_to_tray    = "Alt+Shift+H"
toggle_pin      = "Alt+P"
restore_last    = "Alt+Shift+U"
restore_all     = "Alt+Shift+R"
unstick_keys    = "Alt+Shift+Backspace"

[hider]
service_interval_ms = 750
auto_hide_executables = []
restore_all_on_exit = true

[tray]
tooltip = "Wintil"
icon_path = "assets/icon.ico"

[sentry]
logging_enabled = true
respawn_on_crash = true
max_crashes = 3
crash_window_secs = 60
```

---

## Documentation

- [Detailed Technical Guide](docs/README.md)
- [Automations Manager](docs/Automations%20Manager.md)
- [Changelog](docs/CHANGELOG.md)
