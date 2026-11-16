pub mod audit;

use std::collections::HashMap;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};

use crate::config::keys::{parse_keybind, ParsedKeybind};
use crate::config::HotkeysConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum HotkeyAction {
    ToggleMaximize = 101,
    MinimizeActive = 102,
    CloseActive = 103,
    KillActive = 104,
    KillAllByExe = 105,
    HideToTray = 106,
    TogglePin = 107,
    RestoreLast = 108,
    RestoreAll = 109,
    UnstickKeys = 110,
}

impl HotkeyAction {
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            101 => Some(HotkeyAction::ToggleMaximize),
            102 => Some(HotkeyAction::MinimizeActive),
            103 => Some(HotkeyAction::CloseActive),
            104 => Some(HotkeyAction::KillActive),
            105 => Some(HotkeyAction::KillAllByExe),
            106 => Some(HotkeyAction::HideToTray),
            107 => Some(HotkeyAction::TogglePin),
            108 => Some(HotkeyAction::RestoreLast),
            109 => Some(HotkeyAction::RestoreAll),
            110 => Some(HotkeyAction::UnstickKeys),
            _ => None,
        }
    }
}

pub struct HotkeyManager {
    hwnd: HWND,
    registered: HashMap<i32, ParsedKeybind>,
}

impl HotkeyManager {
    pub fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            registered: HashMap::new(),
        }
    }

    /// Returns the number of currently active registered hotkey hooks
    pub fn registered_count(&self) -> usize {
        self.registered.len()
    }

    /// Registers all hotkeys defined in the configuration
    pub fn register_all(&mut self, config: &HotkeysConfig) {
        self.unregister_all();

        let bindings = [
            (HotkeyAction::ToggleMaximize, &config.maximize_toggle),
            (HotkeyAction::MinimizeActive, &config.minimize_active),
            (HotkeyAction::CloseActive, &config.close_active),
            (HotkeyAction::KillActive, &config.kill_active),
            (HotkeyAction::KillAllByExe, &config.kill_all_by_exe),
            (HotkeyAction::HideToTray, &config.hide_to_tray),
            (HotkeyAction::TogglePin, &config.toggle_pin),
            (HotkeyAction::RestoreLast, &config.restore_last),
            (HotkeyAction::RestoreAll, &config.restore_all),
            (HotkeyAction::UnstickKeys, &config.unstick_keys),
        ];

        for (action, key_str) in bindings {
            if let Some(parsed) = parse_keybind(key_str) {
                let id = action as i32;
                unsafe {
                    let res = RegisterHotKey(
                        self.hwnd,
                        id,
                        parsed.modifiers,
                        parsed.vk.0 as u32,
                    );
                    if res.is_ok() {
                        self.registered.insert(id, parsed);
                    }
                }
            }
        }
    }

    /// Unregisters all registered hotkeys
    pub fn unregister_all(&mut self) {
        for (&id, _) in &self.registered {
            unsafe {
                let _ = UnregisterHotKey(self.hwnd, id);
            }
        }
        self.registered.clear();
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_all();
    }
}
