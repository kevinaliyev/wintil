pub mod daemon;

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Instant;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SetForegroundWindow, ShowWindow, SW_HIDE, SW_SHOW,
};

use crate::window::{get_window_title, process};

#[derive(Debug, Clone)]
pub struct HiddenWindow {
    pub hwnd: isize,
    pub title: String,
    pub exe_name: String,
    pub hidden_at: Instant,
}

#[derive(Clone, Default)]
pub struct HiderStore {
    inner: Arc<Mutex<Vec<HiddenWindow>>>,
}

impl HiderStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Hides the given window from both the desktop and taskbar, adding it to the tracked list
    pub fn hide_window(&self, hwnd: HWND) -> bool {
        unsafe {
            if !IsWindow(hwnd).as_bool() {
                return false;
            }

            let hwnd_val = hwnd.0 as isize;
            let title = get_window_title(hwnd);
            let exe_name = process::get_window_pid(hwnd)
                .and_then(process::get_process_name)
                .unwrap_or_else(|| "Unknown".to_string());

            // Cloak from desktop & taskbar
            let _ = ShowWindow(hwnd, SW_HIDE);

            let mut list = self.inner.lock();
            // Prune duplicate if already tracked
            list.retain(|w| w.hwnd != hwnd_val);
            list.push(HiddenWindow {
                hwnd: hwnd_val,
                title: title.clone(),
                exe_name: exe_name.clone(),
                hidden_at: Instant::now(),
            });
            drop(list);

            crate::sentry::record_cloaked(hwnd_val, title, exe_name);

            true
        }
    }

    /// Restores a specific cloaked window by HWND
    pub fn restore_by_hwnd(&self, hwnd_val: isize) -> bool {
        let mut list = self.inner.lock();
        if let Some(pos) = list.iter().position(|w| w.hwnd == hwnd_val) {
            let item = list.remove(pos);
            drop(list);
            crate::sentry::unrecord_cloaked(hwnd_val);
            unsafe {
                let hwnd = HWND(item.hwnd as _);
                if IsWindow(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            true
        } else {
            false
        }
    }

    /// Restores the most recently hidden window
    pub fn restore_last(&self) -> Option<String> {
        let mut list = self.inner.lock();
        while let Some(item) = list.pop() {
            crate::sentry::unrecord_cloaked(item.hwnd);
            unsafe {
                let hwnd = HWND(item.hwnd as _);
                if IsWindow(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(hwnd);
                    return Some(item.title);
                }
            }
        }
        None
    }

    /// Restores all cloaked windows (used during manual restore or clean exit failsafe)
    pub fn restore_all(&self) -> usize {
        let mut list = self.inner.lock();
        crate::sentry::clear_cloaked();
        let mut restored = 0;
        for item in list.drain(..) {
            unsafe {
                let hwnd = HWND(item.hwnd as _);
                if IsWindow(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    restored += 1;
                }
            }
        }
        restored
    }

    /// Clears the tracking list without restoring windows
    pub fn clear(&self) {
        let mut list = self.inner.lock();
        list.clear();
        drop(list);
        crate::sentry::clear_cloaked();
    }

    /// Prunes dead window handles from the tracking store
    pub fn prune_dead(&self) {
        let mut list = self.inner.lock();
        list.retain(|w| unsafe { IsWindow(HWND(w.hwnd as _)).as_bool() });
    }

    /// Returns a snapshot of currently hidden windows
    pub fn list_hidden(&self) -> Vec<HiddenWindow> {
        self.prune_dead();
        self.inner.lock().clone()
    }

    /// Returns the number of currently tracked hidden windows
    pub fn count(&self) -> usize {
        self.prune_dead();
        self.inner.lock().len()
    }
}
