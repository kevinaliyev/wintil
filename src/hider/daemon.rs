use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, IsWindowVisible};

use crate::hider::HiderStore;
use crate::window::process;

/// Determines if a window's process matches an auto-hide target, correctly distinguishing
/// Chromium forks (e.g. Helium vs Google Chrome) sharing identical binary filenames.
pub fn matches_auto_hide_target(target: &str, exe_name: &str, full_path: &str) -> bool {
    let t = target.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    let exe = exe_name.to_ascii_lowercase();
    let path = full_path.to_ascii_lowercase();

    // 1. Direct path match (if target contains path separators or is exact path)
    if t.contains('\\') || t.contains('/') {
        return path == t || path.ends_with(&t);
    }

    // 2. Helium vs Google Chrome Chromium disambiguation
    let is_helium = path.contains(r"\helium\") || path.contains("imput\\helium");
    let is_google_chrome = path.contains(r"\google\chrome\");

    if t == "helium" || t == "helium.exe" {
        return is_helium;
    }

    if t == "chrome.exe" || t == "chrome" || t == "google chrome" || t == "google-chrome" {
        // "chrome" target ONLY matches Google Chrome / standard Chrome, NEVER Helium!
        return !is_helium && (exe == "chrome.exe" || is_google_chrome);
    }

    // 3. General filename match
    if exe == t {
        return true;
    }

    // 4. Match without .exe extension
    if let Some(stem) = exe.strip_suffix(".exe") {
        if stem == t {
            return true;
        }
    }

    false
}

struct EnumContext<'a> {
    targets: &'a [String],
    to_hide: Vec<HWND>,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumContext);

    // Only inspect top-level windows that are currently visible
    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    if let Some(pid) = process::get_window_pid(hwnd) {
        if let Some(details) = process::get_process_details(pid) {
            for target in ctx.targets {
                if matches_auto_hide_target(target, &details.exe_name, &details.full_path) {
                    ctx.to_hide.push(hwnd);
                    break;
                }
            }
        }
    }

    BOOL(1) // Continue enumeration
}

pub struct AutoHiderService {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AutoHiderService {
    pub fn start(
        store: HiderStore,
        targets: Vec<String>,
        interval_ms: u64,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let interval = Duration::from_millis(interval_ms.max(250));

            while running_clone.load(Ordering::Relaxed) {
                if !targets.is_empty() {
                    let mut ctx = EnumContext {
                        targets: &targets,
                        to_hide: Vec::new(),
                    };

                    unsafe {
                        let _ = EnumWindows(
                            Some(enum_windows_proc),
                            LPARAM(&mut ctx as *mut _ as isize),
                        );
                    }

                    for hwnd in ctx.to_hide {
                        store.hide_window(hwnd);
                    }
                }

                // Chunked sleep so we stop promptly on shutdown
                let chunks = (interval.as_millis() / 100).max(1);
                for _ in 0..chunks {
                    if !running_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AutoHiderService {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_auto_hide_target_chrome_vs_helium() {
        let helium_path = r"C:\Users\ru\AppData\Local\imput\Helium\Application\chrome.exe";
        let chrome_path = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
        let brave_path = r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe";

        // Target: "chrome.exe"
        assert!(matches_auto_hide_target("chrome.exe", "chrome.exe", chrome_path));
        assert!(!matches_auto_hide_target("chrome.exe", "chrome.exe", helium_path));

        // Target: "chrome"
        assert!(matches_auto_hide_target("chrome", "chrome.exe", chrome_path));
        assert!(!matches_auto_hide_target("chrome", "chrome.exe", helium_path));

        // Target: "Helium"
        assert!(matches_auto_hide_target("Helium", "chrome.exe", helium_path));
        assert!(!matches_auto_hide_target("Helium", "chrome.exe", chrome_path));

        // Target: "helium.exe"
        assert!(matches_auto_hide_target("helium.exe", "chrome.exe", helium_path));
        assert!(!matches_auto_hide_target("helium.exe", "chrome.exe", chrome_path));

        // Target: "brave.exe"
        assert!(matches_auto_hide_target("brave.exe", "brave.exe", brave_path));
        assert!(!matches_auto_hide_target("brave.exe", "chrome.exe", chrome_path));
    }

    #[test]
    fn test_matches_auto_hide_target_full_path() {
        let helium_path = r"C:\Users\ru\AppData\Local\imput\Helium\Application\chrome.exe";
        assert!(matches_auto_hide_target(
            r"C:\Users\ru\AppData\Local\imput\Helium\Application\chrome.exe",
            "chrome.exe",
            helium_path
        ));
        assert!(!matches_auto_hide_target(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            "chrome.exe",
            helium_path
        ));
    }
}
