pub mod process;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetCursorPos, GetForegroundWindow, GetShellWindow, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, IsWindow, IsWindowVisible, IsZoomed, PostMessageW,
    ShowWindow, WindowFromPoint, GA_ROOT, SC_CLOSE, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    WM_SYSCOMMAND,
};

/// Retrieves the true visible bounds of a window via DWM, falling back to GetWindowRect
pub fn get_visible_window_bounds(hwnd: HWND) -> Option<RECT> {
    unsafe {
        let mut frame_rect = RECT::default();
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame_rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
        if hr.is_ok()
            && frame_rect.right > frame_rect.left
            && frame_rect.bottom > frame_rect.top
        {
            Some(frame_rect)
        } else {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok()
                && rect.right > rect.left
                && rect.bottom > rect.top
            {
                Some(rect)
            } else {
                None
            }
        }
    }
}

/// Retrieves the active foreground window, if valid
pub fn get_foreground_window() -> Option<HWND> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            None
        } else {
            Some(hwnd)
        }
    }
}

/// Retrieves the window directly under the mouse cursor, falling back to active foreground window
pub fn get_hovered_or_foreground_window() -> Option<HWND> {
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            let hw = WindowFromPoint(pt);
            if !hw.0.is_null() {
                let root = GetAncestor(hw, GA_ROOT);
                if !root.0.is_null() && IsWindow(root).as_bool() && IsWindowVisible(root).as_bool() {
                    let shell = GetShellWindow();
                    if root != shell {
                        return Some(root);
                    }
                }
            }
        }
    }
    get_foreground_window()
}

/// Retrieves the title of a window (or fallback placeholder)
pub fn get_window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len == 0 {
            return "Untitled".to_string();
        }

        let mut buffer = vec![0u16; (len + 1) as usize];
        let copied = GetWindowTextW(hwnd, &mut buffer);
        if copied > 0 {
            String::from_utf16_lossy(&buffer[..copied as usize])
        } else {
            "Untitled".to_string()
        }
    }
}

/// Checks if a window is currently visible to the desktop
pub fn is_visible(hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(hwnd).as_bool() }
}

/// Toggles between Maximize and Restore states
pub fn toggle_maximize(hwnd: HWND) {
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
}

/// Minimizes the window
pub fn minimize(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
    }
}

/// Gracefully closes the window (equivalent to Alt+F4)
pub fn close_window(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(
            hwnd,
            WM_SYSCOMMAND,
            WPARAM(SC_CLOSE as usize),
            LPARAM(0),
        );
    }
}

/// Forcefully kills the process that owns the active window
pub fn kill_active_process(hwnd: HWND) -> Result<String, String> {
    let pid = process::get_window_pid(hwnd)
        .ok_or_else(|| "Could not determine process ID for window".to_string())?;

    let proc_name = process::get_process_name(pid).unwrap_or_else(|| format!("PID {}", pid));
    process::terminate_pid(pid)?;

    Ok(proc_name)
}

/// Forcefully terminates all running processes sharing the active window's executable name
pub fn kill_all_active_by_exe(hwnd: HWND) -> Result<(String, usize), String> {
    let pid = process::get_window_pid(hwnd)
        .ok_or_else(|| "Could not determine process ID for window".to_string())?;

    let proc_name = process::get_process_name(pid)
        .ok_or_else(|| "Could not determine executable name for process".to_string())?;

    let count = process::terminate_all_by_name(&proc_name)?;
    Ok((proc_name, count))
}
