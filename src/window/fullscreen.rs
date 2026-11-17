use std::collections::HashMap;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWCP_DEFAULT, DWMWCP_DONOTROUND, DWMWA_BORDER_COLOR,
    DWMWA_COLOR_DEFAULT, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetShellWindow, GetWindowLongPtrW, GetWindowPlacement, IsWindow, IsWindowVisible, IsZoomed,
    SetWindowLongPtrW, SetWindowPlacement, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_MAXIMIZE, SW_RESTORE, WINDOWPLACEMENT, WS_CAPTION, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME,
    WS_EX_STATICEDGE, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU,
    WS_THICKFRAME,
};

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
            if hwnd.0.is_null() || !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() {
                return false;
            }
            hwnd != GetShellWindow()
        }
    }

    fn apply_fake_fullscreen(hwnd: HWND) -> Option<SavedWindowState> {
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);

            let mut placement = WINDOWPLACEMENT {
                length: std::mem::size_of::<WINDOWPLACEMENT>() as u32,
                ..Default::default()
            };
            if GetWindowPlacement(hwnd, &mut placement).is_err() {
                return None;
            }

            let was_maximized = IsZoomed(hwnd).as_bool();
            if was_maximized {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }

            let chrome_bits = (WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0
                | WS_SYSMENU.0) as isize;
            let ex_chrome_bits = (WS_EX_DLGMODALFRAME.0
                | WS_EX_CLIENTEDGE.0
                | WS_EX_STATICEDGE.0
                | WS_EX_WINDOWEDGE.0) as isize;

            // Switch to WS_POPUP while stripping chrome so Windows removes the 7px
            // invisible DWM resize border and 1px top non-client border cleanly.
            SetWindowLongPtrW(hwnd, GWL_STYLE, (style & !chrome_bits) | (WS_POPUP.0 as isize));
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style & !ex_chrome_bits);

            let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
                // Roll back styles if monitor query fails
                SetWindowLongPtrW(hwnd, GWL_STYLE, style);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style);
                return None;
            }

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
                SWP_FRAMECHANGED | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );

            // Suppress Windows 11 DWM border accent line and corner rounding after frame change.
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

            Some(SavedWindowState {
                style,
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

            SetWindowLongPtrW(hwnd, GWL_STYLE, saved.style);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, saved.ex_style);

            let _ = SetWindowPlacement(hwnd, &saved.placement);
            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOOWNERZORDER
                    | SWP_SHOWWINDOW,
            );

            if saved.was_maximized {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
        }
    }
}
