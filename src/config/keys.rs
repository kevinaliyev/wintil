use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VIRTUAL_KEY,
    VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9,
    VK_A, VK_B, VK_C, VK_D, VK_E, VK_F, VK_G, VK_H, VK_I, VK_J, VK_K, VK_L, VK_M,
    VK_N, VK_O, VK_P, VK_Q, VK_R, VK_S, VK_T, VK_U, VK_V, VK_W, VK_X, VK_Y, VK_Z,
    VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12,
    VK_F13, VK_F14, VK_F15, VK_F16, VK_F17, VK_F18, VK_F19, VK_F2, VK_F20, VK_F21,
    VK_F22, VK_F23, VK_F24, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
    VK_HOME, VK_INSERT, VK_LEFT, VK_NEXT, VK_OEM_MINUS, VK_OEM_PLUS, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB, VK_UP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedKeybind {
    pub modifiers: HOT_KEY_MODIFIERS,
    pub vk: VIRTUAL_KEY,
}

/// Parses human-readable hotkey strings such as:
/// "Alt+F", "Alt+Shift+Q", "Ctrl+Alt+P", "Win+H", "!+q" (AHK shorthand)
pub fn parse_keybind(input: &str) -> Option<ParsedKeybind> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    // Support both delimited style ("Alt+Shift+Q") and AHK-style prefix symbols ("!+q")
    if s.starts_with('!') || s.starts_with('^') || s.starts_with('+') || s.starts_with('#') {
        parse_ahk_shorthand(s)
    } else {
        parse_delimited(s)
    }
}

fn parse_delimited(s: &str) -> Option<ParsedKeybind> {
    let parts: Vec<String> = s.split('+').map(|p| p.trim().to_ascii_lowercase()).collect();
    if parts.is_empty() {
        return None;
    }

    let mut mods = MOD_NOREPEAT.0;
    let mut key_str: Option<&str> = None;

    for part in &parts {
        match part.as_str() {
            "alt" | "menu" => mods |= MOD_ALT.0,
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "windows" | "super" => mods |= MOD_WIN.0,
            "norepeat" => mods |= MOD_NOREPEAT.0,
            other => {
                if key_str.is_none() {
                    key_str = Some(other);
                } else {
                    return None; // multiple primary keys detected
                }
            }
        }
    }

    let key = key_str?;
    let vk = parse_virtual_key(key)?;

    Some(ParsedKeybind {
        modifiers: HOT_KEY_MODIFIERS(mods),
        vk,
    })
}

fn parse_ahk_shorthand(s: &str) -> Option<ParsedKeybind> {
    let mut mods = MOD_NOREPEAT.0;
    let mut chars = s.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '!' => {
                mods |= MOD_ALT.0;
                chars.next();
            }
            '^' => {
                mods |= MOD_CONTROL.0;
                chars.next();
            }
            '+' => {
                mods |= MOD_SHIFT.0;
                chars.next();
            }
            '#' => {
                mods |= MOD_WIN.0;
                chars.next();
            }
            _ => break,
        }
    }

    let remainder: String = chars.collect();
    let vk = parse_virtual_key(&remainder.trim().to_ascii_lowercase())?;

    Some(ParsedKeybind {
        modifiers: HOT_KEY_MODIFIERS(mods),
        vk,
    })
}

fn parse_virtual_key(s: &str) -> Option<VIRTUAL_KEY> {
    let vk = match s {
        // Letters A-Z
        "a" => VK_A,
        "b" => VK_B,
        "c" => VK_C,
        "d" => VK_D,
        "e" => VK_E,
        "f" => VK_F,
        "g" => VK_G,
        "h" => VK_H,
        "i" => VK_I,
        "j" => VK_J,
        "k" => VK_K,
        "l" => VK_L,
        "m" => VK_M,
        "n" => VK_N,
        "o" => VK_O,
        "p" => VK_P,
        "q" => VK_Q,
        "r" => VK_R,
        "s" => VK_S,
        "t" => VK_T,
        "u" => VK_U,
        "v" => VK_V,
        "w" => VK_W,
        "x" => VK_X,
        "y" => VK_Y,
        "z" => VK_Z,

        // Numbers 0-9
        "0" => VK_0,
        "1" => VK_1,
        "2" => VK_2,
        "3" => VK_3,
        "4" => VK_4,
        "5" => VK_5,
        "6" => VK_6,
        "7" => VK_7,
        "8" => VK_8,
        "9" => VK_9,

        // Function keys F1-F24
        "f1" => VK_F1,
        "f2" => VK_F2,
        "f3" => VK_F3,
        "f4" => VK_F4,
        "f5" => VK_F5,
        "f6" => VK_F6,
        "f7" => VK_F7,
        "f8" => VK_F8,
        "f9" => VK_F9,
        "f10" => VK_F10,
        "f11" => VK_F11,
        "f12" => VK_F12,
        "f13" => VK_F13,
        "f14" => VK_F14,
        "f15" => VK_F15,
        "f16" => VK_F16,
        "f17" => VK_F17,
        "f18" => VK_F18,
        "f19" => VK_F19,
        "f20" => VK_F20,
        "f21" => VK_F21,
        "f22" => VK_F22,
        "f23" => VK_F23,
        "f24" => VK_F24,

        // Navigation & control keys
        "space" => VK_SPACE,
        "tab" => VK_TAB,
        "enter" | "return" => VK_RETURN,
        "esc" | "escape" => VK_ESCAPE,
        "backspace" | "back" => VK_BACK,
        "del" | "delete" => VK_DELETE,
        "ins" | "insert" => VK_INSERT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" | "pgup" => VK_PRIOR,
        "pagedown" | "pgdn" => VK_NEXT,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "-" | "minus" => VK_OEM_MINUS,
        "+" | "plus" | "=" | "equal" => VK_OEM_PLUS,

        // Explicit rejection of mouse inputs
        "mouse1" | "mouse2" | "mousekeys1" | "mousekeys2" | "mwheel_down" | "mwheel_up"
        | "mbutton" | "lbutton" | "rbutton" | "xbutton1" | "xbutton2" | "wheelup"
        | "wheeldown" => return None,

        _ => return None,
    };
    Some(vk)
}

/// Returns true if the virtual key corresponds to a mouse button
pub fn is_mouse_virtual_key(vk: VIRTUAL_KEY) -> bool {
    matches!(vk.0, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
}

/// Converts a virtual key into its canonical human-readable string representation
pub fn format_virtual_key(vk: VIRTUAL_KEY) -> Option<&'static str> {
    if is_mouse_virtual_key(vk) {
        return None;
    }
    let s = match vk {
        VK_A => "A",
        VK_B => "B",
        VK_C => "C",
        VK_D => "D",
        VK_E => "E",
        VK_F => "F",
        VK_G => "G",
        VK_H => "H",
        VK_I => "I",
        VK_J => "J",
        VK_K => "K",
        VK_L => "L",
        VK_M => "M",
        VK_N => "N",
        VK_O => "O",
        VK_P => "P",
        VK_Q => "Q",
        VK_R => "R",
        VK_S => "S",
        VK_T => "T",
        VK_U => "U",
        VK_V => "V",
        VK_W => "W",
        VK_X => "X",
        VK_Y => "Y",
        VK_Z => "Z",

        VK_0 => "0",
        VK_1 => "1",
        VK_2 => "2",
        VK_3 => "3",
        VK_4 => "4",
        VK_5 => "5",
        VK_6 => "6",
        VK_7 => "7",
        VK_8 => "8",
        VK_9 => "9",

        VK_F1 => "F1",
        VK_F2 => "F2",
        VK_F3 => "F3",
        VK_F4 => "F4",
        VK_F5 => "F5",
        VK_F6 => "F6",
        VK_F7 => "F7",
        VK_F8 => "F8",
        VK_F9 => "F9",
        VK_F10 => "F10",
        VK_F11 => "F11",
        VK_F12 => "F12",
        VK_F13 => "F13",
        VK_F14 => "F14",
        VK_F15 => "F15",
        VK_F16 => "F16",
        VK_F17 => "F17",
        VK_F18 => "F18",
        VK_F19 => "F19",
        VK_F20 => "F20",
        VK_F21 => "F21",
        VK_F22 => "F22",
        VK_F23 => "F23",
        VK_F24 => "F24",

        VK_SPACE => "Space",
        VK_TAB => "Tab",
        VK_RETURN => "Enter",
        VK_ESCAPE => "Esc",
        VK_BACK => "Backspace",
        VK_DELETE => "Delete",
        VK_INSERT => "Insert",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "PageUp",
        VK_NEXT => "PageDown",
        VK_UP => "Up",
        VK_DOWN => "Down",
        VK_LEFT => "Left",
        VK_RIGHT => "Right",
        VK_OEM_MINUS => "-",
        VK_OEM_PLUS => "+",
        _ => return None,
    };
    Some(s)
}

/// Formats modifiers and virtual key into canonical shortcut string (e.g. "Alt+Shift+P")
pub fn format_hotkey(mods: u32, vk: VIRTUAL_KEY) -> Option<String> {
    if is_mouse_virtual_key(vk) {
        return None;
    }
    let key_name = format_virtual_key(vk)?;
    let mut parts = Vec::new();
    if mods & MOD_ALT.0 != 0 {
        parts.push("Alt");
    }
    if mods & MOD_CONTROL.0 != 0 {
        parts.push("Ctrl");
    }
    if mods & MOD_SHIFT.0 != 0 {
        parts.push("Shift");
    }
    if mods & MOD_WIN.0 != 0 {
        parts.push("Win");
    }
    parts.push(key_name);
    Some(parts.join("+"))
}

/// Checks if `candidate_binding` conflicts with any OTHER action in `hotkeys`.
/// Returns `Some(conflicting_action_display_name)` if a conflict is detected, or `None` if valid.
pub fn find_hotkey_conflict(
    hotkeys: &super::HotkeysConfig,
    action_id: &str,
    candidate_binding: &str,
) -> Option<&'static str> {
    let candidate = parse_keybind(candidate_binding)?;
    let entries: [(&str, &str, &'static str); 11] = [
        ("toggle_pin", &hotkeys.toggle_pin, "Pin Window Always-on-Top"),
        ("hide_to_tray", &hotkeys.hide_to_tray, "Hide Active Window to Tray"),
        ("restore_last", &hotkeys.restore_last, "Restore Last Hidden Window"),
        ("restore_all", &hotkeys.restore_all, "Restore All Hidden Windows"),
        ("maximize_toggle", &hotkeys.maximize_toggle, "Toggle Maximize / Restore"),
        ("fake_fullscreen", &hotkeys.fake_fullscreen, "Toggle Fake Fullscreen"),
        ("minimize_active", &hotkeys.minimize_active, "Minimize Active Window"),
        ("close_active", &hotkeys.close_active, "Close Active Window"),
        ("kill_active", &hotkeys.kill_active, "Force Kill Active Process"),
        ("kill_all_by_exe", &hotkeys.kill_all_by_exe, "Force Kill All Instances"),
        ("unstick_keys", &hotkeys.unstick_keys, "Emergency Unstick All Keys"),
    ];

    for (other_id, other_bind, other_name) in entries {
        if other_id == action_id {
            continue;
        }
        if let Some(other_parsed) = parse_keybind(other_bind) {
            if other_parsed.modifiers == candidate.modifiers && other_parsed.vk == candidate.vk {
                return Some(other_name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_delimited() {
        let kb = parse_keybind("Alt+F").expect("Failed to parse Alt+F");
        assert_eq!(kb.vk, VK_F);
        assert!((kb.modifiers.0 & MOD_ALT.0) != 0);
        assert!((kb.modifiers.0 & MOD_NOREPEAT.0) != 0);

        let kb2 = parse_keybind("Alt+Shift+Q").expect("Failed to parse Alt+Shift+Q");
        assert_eq!(kb2.vk, VK_Q);
        assert!((kb2.modifiers.0 & MOD_ALT.0) != 0);
        assert!((kb2.modifiers.0 & MOD_SHIFT.0) != 0);
    }

    #[test]
    fn test_parse_ahk_shorthand() {
        let kb = parse_keybind("!f").expect("Failed to parse !f");
        assert_eq!(kb.vk, VK_F);
        assert!((kb.modifiers.0 & MOD_ALT.0) != 0);

        let kb2 = parse_keybind("!+q").expect("Failed to parse !+q");
        assert_eq!(kb2.vk, VK_Q);
        assert!((kb2.modifiers.0 & MOD_ALT.0) != 0);
        assert!((kb2.modifiers.0 & MOD_SHIFT.0) != 0);
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_keybind("").is_none());
        assert!(parse_keybind("InvalidKey").is_none());
    }

    #[test]
    fn test_parse_rejects_mouse_inputs() {
        assert!(parse_keybind("mouse1").is_none());
        assert!(parse_keybind("mouse2").is_none());
        assert!(parse_keybind("mousekeys1").is_none());
        assert!(parse_keybind("mousekeys2").is_none());
        assert!(parse_keybind("mwheel_down").is_none());
        assert!(parse_keybind("mwheel_up").is_none());
        assert!(parse_keybind("Alt+mouse1").is_none());
        assert!(parse_keybind("Ctrl+mwheel_down").is_none());
        assert!(is_mouse_virtual_key(VIRTUAL_KEY(0x01))); // VK_LBUTTON
        assert!(is_mouse_virtual_key(VIRTUAL_KEY(0x02))); // VK_RBUTTON
        assert!(is_mouse_virtual_key(VIRTUAL_KEY(0x04))); // VK_MBUTTON
        assert!(!is_mouse_virtual_key(VK_A));
    }

    #[test]
    fn test_format_hotkey_roundtrip() {
        let formatted = format_hotkey(MOD_ALT.0 | MOD_SHIFT.0, VK_P).expect("format Alt+Shift+P");
        assert_eq!(formatted, "Alt+Shift+P");
        let parsed = parse_keybind(&formatted).expect("parse formatted");
        assert_eq!(parsed.vk, VK_P);
        assert!((parsed.modifiers.0 & MOD_ALT.0) != 0);
        assert!((parsed.modifiers.0 & MOD_SHIFT.0) != 0);
    }

    #[test]
    fn test_hotkey_conflict_detection() {
        let cfg = crate::config::HotkeysConfig::default();
        // Trying to bind "toggle_pin" to "Alt+Q" (which is close_active) must return conflict
        let conflict = find_hotkey_conflict(&cfg, "toggle_pin", "Alt+Q");
        assert_eq!(conflict, Some("Close Active Window"));

        // Different modifier ordering ("Shift+Ctrl+Alt+Q" vs "Alt+Ctrl+Shift+Q") must still detect conflict
        let conflict_order = find_hotkey_conflict(&cfg, "toggle_pin", "Shift+Ctrl+Alt+Q");
        assert_eq!(conflict_order, Some("Force Kill All Instances"));

        // Rebinding "toggle_pin" to its own current binding ("Alt+P") must NOT conflict
        assert_eq!(find_hotkey_conflict(&cfg, "toggle_pin", "Alt+P"), None);

        // Unused key combination ("Alt+Shift+Z") must NOT conflict
        assert_eq!(find_hotkey_conflict(&cfg, "toggle_pin", "Alt+Shift+Z"), None);
    }
}
