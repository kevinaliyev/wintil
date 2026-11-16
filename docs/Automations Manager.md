# Wintil Automations Manager

The Automations Manager allows you to declare setup-specific hardware or system fixes, wake handlers, and startup scripts that run seamlessly through Wintil.

## Directory Structure & Portable Mode

By default, Wintil loads and saves user automations in `$HOME\.config\wintil\automations\`.

For portable or USB use, you can also place an `automations\` directory (alongside an optional `wintil.toml`) directly next to `wintil.exe`:

```text
wintil\
├── wintil.exe
├── wintil.toml          # Optional portable config (keybinds, settings)
└── automations\
    └── your-automation-name\
        ├── automation.toml    # Declarative triggers and metadata
        └── fix.ps1            # Source script or executable (.ps1, .bat, .cmd, or .exe)
```

Wintil automatically scans both `$HOME\.config\wintil\automations\` and the `automations\` folder adjacent to `wintil.exe`.

---

## Configuration (`automation.toml`)

Every automation directory must contain an `automation.toml` manifest:

```toml
[automation]
name = "asus-g14-keyboard-wake"
description = "Fix for ASUS Zephyrus G14 (GA402RJ) keyboard sleep/wake and inactivity freeze"
enabled = true

[triggers]
# Run when Wintil boots on startup
on_startup = true

# Run when the system wakes from sleep or Modern Standby (WM_POWERBROADCAST)
on_wake = true

# Run when returning from inactivity / idle state
on_inactivity_resume = true
inactivity_threshold_secs = 60

[execution]
# Name of script inside this automation folder (.ps1, .bat, .cmd, or .exe)
script = "fix.ps1"

[subservice] # Optional: exposes a [ Run Fix ] button under Wintil Services -> Subservices
is_subservice = false
display_name = "My Automation"
```

---

## Supported Triggers

1. **`on_startup`**: Fires immediately when Wintil launches or logs on.
2. **`on_wake`**: Listens for Windows power management events (`WM_POWERBROADCAST` with `PBT_APMRESUMEAUTOMATIC` / `PBT_APMRESUMESUSPEND`). Automatically clears latched key states and executes the fix before you begin typing.
3. **`on_inactivity_resume`**: Uses Win32 `GetLastInputInfo` to monitor global user idle time. When user input resumes after being inactive for `>= inactivity_threshold_secs`, Wintil triggers the automation script in the background.
4. **Manual Trigger**: Run from terminal using `wintil.exe --run-automation <name>` or via the Wintil Quick Settings flyout.

---

## Execution & Logging

- Scripts are executed detached in the background with `CREATE_NO_WINDOW`, so no command prompt or PowerShell windows steal focus or interrupt work.
- Execution history and status are logged to `$HOME/.config/wintil/logs/automations.log`.
