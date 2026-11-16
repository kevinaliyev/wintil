pub mod hud;
pub mod stroke;

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, IsWindow, SetWindowPos, SetWindowTextW, GWL_EXSTYLE, HWND_NOTOPMOST,
    HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOPMOST,
};

use crate::window::get_window_title;

const PINNED_TAG: &str = " [Pinned]";

#[derive(Clone, Default)]
pub struct PinStore {
    inner: Arc<Mutex<HashSet<isize>>>,
}

impl PinStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Toggles the Always-On-Top state for a window and optionally tags its title
    pub fn toggle_pin(&self, hwnd: HWND, tag_title: bool) -> Result<bool, String> {
        let is_now_pinned = toggle_pin_window(hwnd)?;
        let val = hwnd.0 as isize;

        if tag_title {
            update_title_tag(hwnd, is_now_pinned);
        }

        let mut set = self.inner.lock();
        if is_now_pinned {
            set.insert(val);
            drop(set);
            let cur_title = get_window_title(hwnd);
            let orig = cur_title.strip_suffix(PINNED_TAG).unwrap_or(&cur_title).to_string();
            crate::sentry::record_pinned(val, orig);
        } else {
            set.remove(&val);
            drop(set);
            crate::sentry::unrecord_pinned(val);
        }

        Ok(is_now_pinned)
    }

    /// Checks if a window is tracked as pinned
    pub fn is_tracked_pinned(&self, hwnd_val: isize) -> bool {
        self.inner.lock().contains(&hwnd_val)
    }

    /// Explicitly unpins a window by HWND
    pub fn unpin_by_hwnd(&self, hwnd_val: isize, tag_title: bool) -> bool {
        unsafe {
            let hwnd = HWND(hwnd_val as _);
            if IsWindow(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE,
                );
                if tag_title {
                    update_title_tag(hwnd, false);
                }
            }
        }
        crate::sentry::unrecord_pinned(hwnd_val);
        self.inner.lock().remove(&hwnd_val)
    }

    /// Prunes invalid/closed windows from tracking
    pub fn prune_dead(&self) {
        let mut set = self.inner.lock();
        set.retain(|&val| unsafe { IsWindow(HWND(val as _)).as_bool() });
    }

    /// Returns a list of currently tracked pinned window HWNDs
    pub fn list_pinned(&self) -> Vec<isize> {
        self.prune_dead();
        self.inner.lock().iter().copied().collect()
    }
}

/// Toggles Always-On-Top state directly via Win32 API
pub fn toggle_pin_window(hwnd: HWND) -> Result<bool, String> {
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let is_topmost = (ex_style & (WS_EX_TOPMOST.0 as isize)) != 0;

        let insert_after = if is_topmost {
            HWND_NOTOPMOST
        } else {
            HWND_TOPMOST
        };

        let success = SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );

        if success.is_ok() {
            Ok(!is_topmost)
        } else {
            Err("Failed to set window topmost state".to_string())
        }
    }
}

/// Appends or removes "[Pinned]" from window title to provide taskbar/titlebar visibility
fn update_title_tag(hwnd: HWND, pinned: bool) {
    unsafe {
        let current_title = get_window_title(hwnd);
        if pinned {
            if !current_title.ends_with(PINNED_TAG) {
                let new_title = format!("{}{}", current_title, PINNED_TAG);
                let wide: Vec<u16> = new_title.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            }
        } else {
            if let Some(stripped) = current_title.strip_suffix(PINNED_TAG) {
                let wide: Vec<u16> = stripped.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
            }
        }
    }
}
