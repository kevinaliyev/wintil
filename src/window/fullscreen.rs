use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWCP_DEFAULT, DWMWCP_DONOTROUND, DWMWA_BORDER_COLOR,
    DWMWA_COLOR_DEFAULT, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetShellWindow, GetWindowLongPtrW, GetWindowPlacement, GetWindowRect, IsIconic,
    IsWindow, IsWindowVisible, IsZoomed, SendMessageTimeoutW, SetWindowLongPtrW,
    SetWindowPlacement, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE, SMTO_ABORTIFHUNG,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SW_MAXIMIZE, WINDOWPLACEMENT, WM_NULL, WS_CAPTION, WS_CHILD,
    WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_WINDOWEDGE, WS_MAXIMIZE,
    WS_MAXIMIZEBOX, WS_MINIMIZE, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

const CHROME_STYLE_BITS: isize = (WS_CAPTION.0
    | WS_THICKFRAME.0
    | WS_MINIMIZEBOX.0
    | WS_MAXIMIZEBOX.0
    | WS_SYSMENU.0
    | WS_MAXIMIZE.0
    | WS_MINIMIZE.0) as isize;

const CHROME_EX_STYLE_BITS: isize = (WS_EX_DLGMODALFRAME.0
    | WS_EX_CLIENTEDGE.0
    | WS_EX_STATICEDGE.0
    | WS_EX_WINDOWEDGE.0) as isize;

const STATE_STYLE_BITS: isize = (WS_MAXIMIZE.0 | WS_MINIMIZE.0) as isize;

fn clean_saved_style(style: isize) -> isize {
    style & !STATE_STYLE_BITS
}

fn compute_fake_fullscreen_styles(style: isize, ex_style: isize) -> (isize, isize) {
    let new_style = style & !CHROME_STYLE_BITS;
    let new_ex_style = ex_style & !CHROME_EX_STYLE_BITS;
    (new_style, new_ex_style)
}

fn is_borderless_fullscreen_rect(style: isize, rect: RECT, monitor_rect: RECT) -> bool {
    let has_chrome = (style & ((WS_CAPTION.0 | WS_THICKFRAME.0) as isize)) != 0;
    !has_chrome
        && rect.left <= monitor_rect.left
        && rect.top <= monitor_rect.top
        && rect.right >= monitor_rect.right
        && rect.bottom >= monitor_rect.bottom
}

struct SavedWindowState {
    style: isize,
    ex_style: isize,
    placement: WINDOWPLACEMENT,
    was_maximized: bool,
}

/// Tracks windows currently in pseudo-fullscreen (borderless work-area) mode
/// so their original styles and placement can be cleanly restored.
pub struct FakeFullscreenStore {
    windows: HashMap<isize, SavedWindowState>,
}

impl FakeFullscreenStore {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    /// Toggles pseudo-fullscreen on `hwnd`. Returns `true` if entered, `false` if exited.
    pub fn toggle(&mut self, hwnd: HWND) -> bool {
        self.prune_dead();

        let key = hwnd.0 as isize;
        if let Some(saved) = self.windows.remove(&key) {
            Self::restore_window(hwnd, saved);
            return false;
        }

        if !Self::is_eligible(hwnd) {
            return false;
        }

        if let Some(saved) = Self::apply_fake_fullscreen(hwnd) {
            self.windows.insert(key, saved);
            return true;
        }

        false
    }

    /// Exits pseudo-fullscreen on `hwnd` if it is currently tracked. Returns `true` if restored.
    pub fn exit(&mut self, hwnd: HWND) -> bool {
        self.prune_dead();
        let key = hwnd.0 as isize;
        if let Some(saved) = self.windows.remove(&key) {
            Self::restore_window(hwnd, saved);
            return true;
        }
        false
    }

    /// Restores all tracked pseudo-fullscreen windows (e.g. on application exit).
    pub fn restore_all(&mut self) {
        let entries: Vec<(isize, SavedWindowState)> = self.windows.drain().collect();
        for (key, saved) in entries {
            let hwnd = HWND(key as _);
            Self::restore_window(hwnd, saved);
        }
    }

    fn prune_dead(&mut self) {
        self.windows
            .retain(|&key, _| unsafe { IsWindow(HWND(key as _)).as_bool() });
    }

    fn is_eligible(hwnd: HWND) -> bool {
        unsafe {
            if hwnd.0.is_null()
                || !IsWindow(hwnd).as_bool()
                || !IsWindowVisible(hwnd).as_bool()
                || IsIconic(hwnd).as_bool()
            {
                return false;
            }
            if hwnd == GetShellWindow() {
                return false;
            }
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            if (style & (WS_CHILD.0 as isize)) != 0 {
                return false;
            }
            let mut class_buf = [0u16; 64];
            let len = GetClassNameW(hwnd, &mut class_buf);
            if len > 0 {
                let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
                if matches!(
                    class_name.as_str(),
                    "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Progman" | "WorkerW"
                ) {
                    return false;
                }
            }
            true
        }
    }

    fn apply_fake_fullscreen(hwnd: HWND) -> Option<SavedWindowState> {
        unsafe {
            let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
                return None;
            }

            let initial_style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let mut current_rect = RECT::default();
            if GetWindowRect(hwnd, &mut current_rect).is_ok()
                && is_borderless_fullscreen_rect(initial_style, current_rect, mi.rcMonitor)
            {
                // Window is already in native/borderless fullscreen; avoid saving its
                // borderless state as the restore baseline.
                return None;
            }

            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(hwnd, &mut placement).is_err() {
                return None;
            }

            let was_maximized = IsZoomed(hwnd).as_bool();

            // Capture original styles before maximizing so restore_window returns to the
            // exact pre-fullscreen style and placement.
            let raw_style = initial_style;
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let saved_style = clean_saved_style(raw_style);
            let (new_style, new_ex_style) = compute_fake_fullscreen_styles(saved_style, ex_style);

            // If the window is not already maximized (e.g. spawned extending below the taskbar),
            // maximize it first with chrome intact so Windows and XAML/DirectComposition apps
            // reflow cleanly to rcWork before stripping chrome.
            if !was_maximized {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                let _ = SendMessageTimeoutW(
                    hwnd,
                    WM_NULL,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_ABORTIFHUNG,
                    100,
                    None,
                );
            }

            // Suppress Windows 11 DWM border accent line and corner rounding before committing frame.
            let no_border = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &no_border as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            let no_round = DWMWCP_DONOTROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &no_round as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );

            SetWindowLongPtrW(hwnd, GWL_STYLE, new_style);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style);

            // Use rcWork to respect the taskbar; subtract 1px height so DWM never enters
            // fullscreen occlusion / Direct Flip.
            let x = mi.rcWork.left;
            let y = mi.rcWork.top;
            let w = (mi.rcWork.right - mi.rcWork.left).max(1);
            let h = (mi.rcWork.bottom - mi.rcWork.top - 1).max(1);

            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                x,
                y,
                w,
                h,
                SWP_FRAMECHANGED
                    | SWP_NOCOPYBITS
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_SHOWWINDOW,
            );
            let _ = RedrawWindow(
                hwnd,
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
            );

            Some(SavedWindowState {
                style: saved_style,
                ex_style,
                placement,
                was_maximized,
            })
        }
    }

    fn restore_window(hwnd: HWND, saved: SavedWindowState) {
        unsafe {
            if !IsWindow(hwnd).as_bool() {
                return;
            }

            let default_border = DWMWA_COLOR_DEFAULT;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &default_border as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            let default_round = DWMWCP_DEFAULT;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &default_round as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );

            SetWindowLongPtrW(hwnd, GWL_STYLE, clean_saved_style(saved.style));
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, saved.ex_style);

            // Commit the restored non-client frame metrics before applying placement.
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOCOPYBITS
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_NOACTIVATE,
            );

            let _ = SetWindowPlacement(hwnd, &saved.placement);
            if saved.was_maximized {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }

            let _ = RedrawWindow(
                hwnd,
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_ALLCHILDREN,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
    };

    #[test]
    fn test_compute_fake_fullscreen_styles_strips_maximize_and_chrome() {
        let style = (WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0 | WS_MAXIMIZE.0) as isize;
        let ex_style = (WS_EX_APPWINDOW.0 | WS_EX_WINDOWEDGE.0 | WS_EX_CLIENTEDGE.0) as isize;

        let (new_style, new_ex) = compute_fake_fullscreen_styles(style, ex_style);

        assert_eq!(new_style & (WS_POPUP.0 as isize), 0);
        assert_ne!(new_style & (WS_VISIBLE.0 as isize), 0);
        assert_eq!(new_style & (WS_MAXIMIZE.0 as isize), 0);
        assert_eq!(new_style & (WS_MINIMIZE.0 as isize), 0);
        assert_eq!(new_style & (WS_CAPTION.0 as isize), 0);
        assert_eq!(new_style & (WS_THICKFRAME.0 as isize), 0);
        assert_ne!(new_ex & (WS_EX_APPWINDOW.0 as isize), 0);
        assert_eq!(new_ex & (WS_EX_WINDOWEDGE.0 as isize), 0);
        assert_eq!(new_ex & (WS_EX_CLIENTEDGE.0 as isize), 0);
    }

    #[test]
    fn test_clean_saved_style_clears_zoom_bits() {
        let style = (WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0 | WS_MAXIMIZE.0 | WS_MINIMIZE.0) as isize;
        let cleaned = clean_saved_style(style);
        assert_eq!(cleaned & (WS_MAXIMIZE.0 as isize), 0);
        assert_eq!(cleaned & (WS_MINIMIZE.0 as isize), 0);
        assert_eq!(cleaned & (WS_OVERLAPPEDWINDOW.0 as isize), WS_OVERLAPPEDWINDOW.0 as isize);
    }

    #[test]
    fn test_is_borderless_fullscreen_rect_detection() {
        let monitor = RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let popup_style = (WS_POPUP.0 | WS_VISIBLE.0) as isize;
        let overlapped_style = (WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0) as isize;

        assert!(is_borderless_fullscreen_rect(popup_style, monitor, monitor));
        assert!(!is_borderless_fullscreen_rect(overlapped_style, monitor, monitor));
        assert!(!is_borderless_fullscreen_rect(
            popup_style,
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1031
            },
            monitor
        ));
    }
}
