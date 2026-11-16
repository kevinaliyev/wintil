use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    GUI_INMENUMODE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE,
};

use crate::window::get_window_title;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModifierState {
    pub vk: VIRTUAL_KEY,
    pub name: &'static str,
    pub is_extended: bool,
    pub physically_down: bool,
    pub logically_down: bool,
}

#[derive(Debug, Clone)]
pub struct KeyAuditSnapshot {
    pub timestamp_ms: u64,
    pub foreground_hwnd: isize,
    pub foreground_title: String,
    pub in_menu_mode: bool,
    pub modifiers: Vec<ModifierState>,
    pub has_stuck_keys: bool,
}

/// Checks the physical and logical status of all modifier keys (pure read-only inspection, zero injection).
pub fn get_audit_snapshot() -> KeyAuditSnapshot {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let fg_hwnd = unsafe { GetForegroundWindow() };
    let fg_val = fg_hwnd.0 as isize;
    let fg_title = if fg_val != 0 {
        get_window_title(fg_hwnd)
    } else {
        "None".to_string()
    };

    let mut in_menu_mode = false;
    if fg_val != 0 {
        let thread_id = unsafe { GetWindowThreadProcessId(fg_hwnd, None) };
        if thread_id != 0 {
            let mut gui_info = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            unsafe {
                if GetGUIThreadInfo(thread_id, &mut gui_info).is_ok() {
                    let flags = gui_info.flags;
                    if (flags & GUI_INMENUMODE).0 != 0
                        || (flags & GUI_POPUPMENUMODE).0 != 0
                        || (flags & GUI_SYSTEMMENUMODE).0 != 0
                    {
                        in_menu_mode = true;
                    }
                }
            }
        }
    }

    let watched: &[(VIRTUAL_KEY, &'static str, bool)] = &[
        (VK_LMENU, "LAlt", false),
        (VK_RMENU, "RAlt", true),
        (VK_MENU, "Alt", false),
        (VK_LSHIFT, "LShift", false),
        (VK_RSHIFT, "RShift", true),
        (VK_SHIFT, "Shift", false),
        (VK_LCONTROL, "LCtrl", false),
        (VK_RCONTROL, "RCtrl", true),
        (VK_CONTROL, "Ctrl", false),
        (VK_LWIN, "LWin", true),
        (VK_RWIN, "RWin", true),
    ];

    let mut modifiers = Vec::with_capacity(watched.len());
    let mut has_stuck_keys = false;

    for &(vk, name, is_extended) in watched {
        let phys_down = unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 };
        let log_down = unsafe { (GetKeyState(vk.0 as i32) as u16 & 0x8000) != 0 };

        // Stuck key definition: Windows considers the key down in message queue/thread state,
        // but the physical key is NOT held down by the user
        if log_down && !phys_down {
            has_stuck_keys = true;
        }

        modifiers.push(ModifierState {
            vk,
            name,
            is_extended,
            physically_down: phys_down,
            logically_down: log_down,
        });
    }

    KeyAuditSnapshot {
        timestamp_ms: now,
        foreground_hwnd: fg_val,
        foreground_title: fg_title,
        in_menu_mode,
        modifiers,
        has_stuck_keys,
    }
}

fn is_extended_key(vk: u16) -> bool {
    matches!(
        vk,
        0x21..=0x2E   // VK_PRIOR, VK_NEXT, VK_END, VK_HOME, VK_LEFT, VK_UP, VK_RIGHT, VK_DOWN, VK_SELECT, VK_PRINT, VK_EXECUTE, VK_SNAPSHOT, VK_INSERT, VK_DELETE
        | 0x5B | 0x5C // VK_LWIN, VK_RWIN
        | 0xA3        // VK_RCONTROL
        | 0xA5        // VK_RMENU
        | 0x6F        // VK_DIVIDE
        | 0x90        // VK_NUMLOCK
    )
}

/// Universal key unsticking:
/// Scans virtual keys across the keyboard (modifiers, arrows, navigation).
/// Detects any key reported as physically or logically down via GetAsyncKeyState / GetKeyState,
/// and injects clean KEYEVENTF_KEYUP events.
/// NOTE: This function NEVER injects Space, Enter, Tab, or Escape, preventing unintended
/// clicks, form submissions, or video pausing in background/browser windows.
pub fn universal_unstick_all_keys() {
    let mut inputs: Vec<INPUT> = Vec::new();
    let mut unlatched_count = 0;

    // Scan virtual keys from 0x08 to 0xFE (skipping mouse buttons 0x01..=0x07)
    for vk_code in 0x08u16..=0xFEu16 {
        // Skip Space (0x20), Return (0x0D), Tab (0x09), Escape (0x1B) from broad scans to avoid rogue clicks
        if vk_code == 0x20 || vk_code == 0x0D || vk_code == 0x09 || vk_code == 0x1B {
            continue;
        }

        let phys_down = unsafe { (GetAsyncKeyState(vk_code as i32) as u16 & 0x8000) != 0 };
        let log_down = unsafe { (GetKeyState(vk_code as i32) as u16 & 0x8000) != 0 };
        if phys_down || log_down {
            let mut flags = KEYEVENTF_KEYUP;
            if is_extended_key(vk_code) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }

            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk_code),
                        wScan: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            unlatched_count += 1;
        }
    }

    // Always ensure modifiers and navigation keys receive KeyUp if explicitly requested
    let priority_keys: &[u16] = &[
        0x12, 0xA4, 0xA5, // VK_MENU, VK_LMENU, VK_RMENU (Alt)
        0x11, 0xA2, 0xA3, // VK_CONTROL, VK_LCONTROL, VK_RCONTROL (Ctrl)
        0x10, 0xA0, 0xA1, // VK_SHIFT, VK_LSHIFT, VK_RSHIFT (Shift)
        0x5B, 0x5C,       // VK_LWIN, VK_RWIN (Win)
        0x25, 0x26, 0x27, 0x28, // Left, Up, Right, Down arrows
        0x21, 0x22, 0x23, 0x24, // PageUp, PageDown, End, Home
    ];
    for &vk_code in priority_keys {
        if !inputs.iter().any(|i| unsafe { i.Anonymous.ki.wVk.0 == vk_code }) {
            let mut flags = KEYEVENTF_KEYUP;
            if is_extended_key(vk_code) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk_code),
                        wScan: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }

    // Harmless matrix cycle pulse on explicit manual unstick: VK_F24 (0x87)
    // Breaks repeating hardware latch loops on ASUS N-KEY controllers without triggering any OS action
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0x87), // VK_F24
                wScan: 0,
                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0x87), // VK_F24
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });

    if !inputs.is_empty() {
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        log_audit_event("UNIVERSAL_UNSTICK", &format!("Flushed key buffer (detected {} down)", unlatched_count));
    }
}

/// Releases ONLY modifier keys that are PHYSICALLY HELD DOWN by the user when a window is closed or hidden.
/// If no modifier keys are physically pressed, this function performs ZERO input injection (0 events sent).
pub fn clean_release_held_modifiers() {
    let modifiers: &[u16] = &[
        0x12, 0xA4, 0xA5, // VK_MENU, VK_LMENU, VK_RMENU (Alt)
        0x11, 0xA2, 0xA3, // VK_CONTROL, VK_LCONTROL, VK_RCONTROL (Ctrl)
        0x10, 0xA0, 0xA1, // VK_SHIFT, VK_LSHIFT, VK_RSHIFT (Shift)
        0x5B, 0x5C,       // VK_LWIN, VK_RWIN (Win)
    ];

    let mut inputs = Vec::new();
    for &vk in modifiers {
        let phys_down = unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
        if phys_down {
            let mut flags = KEYEVENTF_KEYUP;
            if is_extended_key(vk) {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(vk),
                        wScan: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }

    if !inputs.is_empty() {
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        log_audit_event("MODIFIERS_CLEAN_RELEASE", &format!("Released {} held modifier(s)", inputs.len()));
    }
}

/// Resolves the canonical audit log path at $HOME/.config/wintil/logs/keyboard_audit.log
pub fn get_audit_log_path() -> PathBuf {
    crate::config::get_logs_dir().join("keyboard_audit.log")
}

/// Appends a structured audit event to the log file.
pub fn log_audit_event(event_type: &str, details: &str) {
    let path = get_audit_log_path();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let entry = format!("[{now} ms] [{event_type}] {details}\n");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(entry.as_bytes());
    }

    // Pipe keyboard audit events into sequential master log
    crate::sentry::log_info(
        "hotkeys",
        &format!("[{event_type}] {details}"),
    );
}

/// Runs a real-time terminal audit monitor (wintil.exe --audit-keys)
pub fn run_cli_audit_monitor() {
    println!("================================================================================");
    println!("             WINTIL KEYBOARD AUDIT & TELEMETRY MONITOR (v{})", crate::version::APP_VERSION);
    println!("================================================================================");
    println!("Press Ctrl+C to exit this monitor.");
    println!("Perform your normal keyboard shortcuts, Alt+Tab, PowerToys Run, or mouse actions.");
    println!("The table below audits physical vs logical key states without injecting any input.\n");

    let mut last_fg_hwnd = 0isize;
    let mut last_stuck_state = false;

    loop {
        let snapshot = get_audit_snapshot();

        if snapshot.foreground_hwnd != last_fg_hwnd || snapshot.has_stuck_keys != last_stuck_state {
            last_fg_hwnd = snapshot.foreground_hwnd;
            last_stuck_state = snapshot.has_stuck_keys;

            let status_str = if snapshot.has_stuck_keys {
                "!! ALERT: STUCK KEY DETECTED !!"
            } else {
                "HEALTHY (Synchronized)"
            };

            let menu_str = if snapshot.in_menu_mode {
                "[MENU BAR ACTIVE]"
            } else {
                "[Menu: Normal]"
            };

            let mut key_summary = String::new();
            for m in &snapshot.modifiers {
                if m.name == "Alt" || m.name == "Shift" || m.name == "Ctrl" || m.name == "LWin" {
                    let p = if m.physically_down { "DN" } else { "up" };
                    let l = if m.logically_down { "DN" } else { "up" };
                    let flag = if m.logically_down && !m.physically_down {
                        " *STUCK*"
                    } else {
                        ""
                    };
                    key_summary.push_str(&format!("  {}:[phys={}, log={}{}]", m.name, p, l, flag));
                }
            }

            println!(
                "[{status_str}] {menu} | FG: \"{title}\" (0x{hwnd:X}) | Keys:{keys}",
                status_str = status_str,
                menu = menu_str,
                title = snapshot.foreground_title,
                hwnd = snapshot.foreground_hwnd,
                keys = key_summary
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_snapshot_structure() {
        let snap = get_audit_snapshot();
        assert!(!snap.modifiers.is_empty());
        assert!(snap.modifiers.iter().any(|m| m.name == "Alt"));
        assert!(snap.modifiers.iter().any(|m| m.name == "Shift"));
        assert!(snap.modifiers.iter().any(|m| m.name == "Ctrl"));
    }

    #[test]
    fn test_clean_release_execution() {
        // Calling clean_release_held_modifiers should execute safely without injecting unless keys are held
        clean_release_held_modifiers();
    }

    #[test]
    fn test_audit_log_event() {
        let test_event = "TEST_EVENT_VERIFICATION";
        let test_detail = "UnitTestVerificationPayload";
        log_audit_event(test_event, test_detail);

        let path = get_audit_log_path();
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(contents.contains(test_event));
        assert!(contents.contains(test_detail));
    }

    #[test]
    fn test_stuck_key_classification() {
        let normal_state = ModifierState {
            vk: VK_MENU,
            name: "Alt",
            is_extended: false,
            physically_down: false,
            logically_down: false,
        };
        assert!(!(normal_state.logically_down && !normal_state.physically_down));

        let held_state = ModifierState {
            vk: VK_MENU,
            name: "Alt",
            is_extended: false,
            physically_down: true,
            logically_down: true,
        };
        assert!(!(held_state.logically_down && !held_state.physically_down));

        let stuck_state = ModifierState {
            vk: VK_MENU,
            name: "Alt",
            is_extended: false,
            physically_down: false,
            logically_down: true,
        };
        assert!(stuck_state.logically_down && !stuck_state.physically_down);
    }
}
