use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, InvalidateRect, SetWindowRgn, HBRUSH, HDC, PAINTSTRUCT, RGN_DIFF,
};
use windows::Win32::UI::Accessibility::{
    SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetClassLongPtrW,
    GetWindowRect, IsIconic, IsWindow, IsWindowVisible, IsZoomed, RegisterClassW, SetClassLongPtrW,
    SetWindowPos, ShowWindow, GCLP_HBRBACKGROUND, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOOWNERZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_PAINT,
    WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const STROKE_CLASS_NAME: PCWSTR = w!("WintilStrokeOverlayClass");
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StrokeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_stroke_thickness")]
    pub thickness: i32,
    #[serde(default = "default_stroke_color")]
    pub color: String,
}

fn default_stroke_thickness() -> i32 {
    2
}

fn default_stroke_color() -> String {
    "burgundy".to_string()
}

impl Default for StrokeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            thickness: 2,
            color: "burgundy".to_string(),
        }
    }
}

impl StrokeConfig {
    pub fn to_colorref(&self) -> COLORREF {
        let (r, g, b) = crate::config::parse_color_rgb(&self.color, (128.0, 20.0, 42.0));
        let r_u = r as u32;
        let g_u = g as u32;
        let b_u = b as u32;
        COLORREF((b_u << 16) | (g_u << 8) | r_u)
    }
}

unsafe extern "system" fn stroke_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let hdc = HDC(wparam.0 as _);
            let brush = GetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND);
            if brush != 0 {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = FillRect(hdc, &rect, HBRUSH(brush as _));
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let brush = GetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND);
            if brush != 0 {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let _ = FillRect(hdc, &rect, HBRUSH(brush as _));
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

struct OverlayInstance {
    stroke_hwnd: isize,
    current_rect: RECT,
    last_brush: isize,
}

#[derive(Clone)]
pub struct StrokeManager {
    inner: Arc<Mutex<StrokeState>>,
}

struct StrokeState {
    config: StrokeConfig,
    overlays: HashMap<isize, OverlayInstance>,
    hook: Option<isize>,
}

static GLOBAL_STROKE_STATE: parking_lot::RwLock<Option<Arc<Mutex<StrokeState>>>> =
    parking_lot::RwLock::new(None);

fn get_scaled_thickness(target_hwnd: HWND, base_thickness: i32) -> i32 {
    let dpi = unsafe {
        let d = windows::Win32::UI::HiDpi::GetDpiForWindow(target_hwnd);
        if d == 0 {
            let s = windows::Win32::UI::HiDpi::GetDpiForSystem();
            if s == 0 { 96 } else { s }
        } else {
            d
        }
    };
    let scaled = ((base_thickness.clamp(1, 8) as i64 * dpi as i64 + 48) / 96) as i32;
    scaled.max(1)
}

fn get_visible_window_bounds(target_hwnd: HWND) -> Option<RECT> {
    unsafe {
        let mut frame_rect = RECT::default();
        let hr = DwmGetWindowAttribute(
            target_hwnd,
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
            if GetWindowRect(target_hwnd, &mut rect).is_ok()
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

fn get_overlay_geometry(target_hwnd: HWND, thickness: i32) -> Option<(i32, i32, i32, i32, RECT)> {
    let visible_rect = get_visible_window_bounds(target_hwnd)?;
    let v_w = (visible_rect.right - visible_rect.left).max(10);
    let v_h = (visible_rect.bottom - visible_rect.top).max(10);

    let is_maximized = unsafe { IsZoomed(target_hwnd).as_bool() };
    let (ox, oy, ow, oh) = if is_maximized {
        (visible_rect.left, visible_rect.top, v_w, v_h)
    } else {
        (
            visible_rect.left - thickness,
            visible_rect.top - thickness,
            v_w + 2 * thickness,
            v_h + 2 * thickness,
        )
    };

    Some((ox, oy, ow, oh, visible_rect))
}

unsafe extern "system" fn win_event_hook_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _id_thread: u32,
    _time: u32,
) {
    if event == EVENT_OBJECT_LOCATIONCHANGE && id_object == 0 && !hwnd.0.is_null() {
        let hwnd_val = hwnd.0 as isize;
        if let Some(state_arc) = GLOBAL_STROKE_STATE.read().as_ref() {
            let mut state = state_arc.lock();
            if state.config.enabled {
                let thickness = get_scaled_thickness(hwnd, state.config.thickness);
                if let Some(inst) = state.overlays.get_mut(&hwnd_val) {
                    update_overlay_pos(hwnd, inst, thickness);
                }
            }
        }
    }
}

unsafe fn update_overlay_pos(target_hwnd: HWND, inst: &mut OverlayInstance, thickness: i32) {
    let stroke_h = HWND(inst.stroke_hwnd as _);
    if !IsWindow(target_hwnd).as_bool() || !IsWindowVisible(target_hwnd).as_bool() || IsIconic(target_hwnd).as_bool() {
        let _ = ShowWindow(stroke_h, SW_HIDE);
        return;
    }

    if let Some((ox, oy, ow, oh, visible_rect)) = get_overlay_geometry(target_hwnd, thickness) {
        if visible_rect.left != inst.current_rect.left
            || visible_rect.top != inst.current_rect.top
            || visible_rect.right != inst.current_rect.right
            || visible_rect.bottom != inst.current_rect.bottom
        {
            let old_w = inst.current_rect.right - inst.current_rect.left;
            let old_h = inst.current_rect.bottom - inst.current_rect.top;
            let new_w = visible_rect.right - visible_rect.left;
            let new_h = visible_rect.bottom - visible_rect.top;

            inst.current_rect = visible_rect;

            // Only rebuild GDI region if width or height changed
            if new_w != old_w || new_h != old_h {
                apply_hollow_region(stroke_h, ow, oh, thickness);
            }

            let _ = SetWindowPos(
                stroke_h,
                HWND_TOPMOST,
                ox,
                oy,
                ow,
                oh,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(stroke_h, SW_SHOWNOACTIVATE);
        }
    }
}

impl StrokeState {
    fn ensure_hook(&mut self) {
        if self.config.enabled && !self.overlays.is_empty() && self.hook.is_none() {
            unsafe {
                let hook = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    HMODULE(std::ptr::null_mut()),
                    Some(win_event_hook_proc),
                    0,
                    0,
                    0,
                );
                if !hook.is_invalid() {
                    self.hook = Some(hook.0 as isize);
                }
            }
        }
    }

    fn release_hook_if_idle(&mut self) {
        if (!self.config.enabled || self.overlays.is_empty()) && self.hook.is_some() {
            if let Some(h) = self.hook.take() {
                unsafe {
                    let _ = UnhookWinEvent(HWINEVENTHOOK(h as _));
                }
            }
        }
    }
}

impl StrokeManager {
    pub fn new(config: StrokeConfig) -> Self {
        let state = Arc::new(Mutex::new(StrokeState {
            config,
            overlays: HashMap::new(),
            hook: None,
        }));

        *GLOBAL_STROKE_STATE.write() = Some(state.clone());

        // Register Win32 class for stroke overlay windows
        unsafe {
            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(stroke_wnd_proc),
                lpszClassName: STROKE_CLASS_NAME,
                ..Default::default()
            };
            let _ = RegisterClassW(&wnd_class);
        }

        Self { inner: state }
    }

    /// Adds or updates a stroke overlay for a pinned target window
    pub fn attach_stroke(&self, target_hwnd: HWND) {
        let mut state = self.inner.lock();
        if !state.config.enabled {
            return;
        }

        let hwnd_val = target_hwnd.0 as isize;
        if state.overlays.contains_key(&hwnd_val) {
            return;
        }

        unsafe {
            if !IsWindow(target_hwnd).as_bool() {
                return;
            }

            let thickness = get_scaled_thickness(target_hwnd, state.config.thickness);
            let (ox, oy, ow, oh, visible_rect) = match get_overlay_geometry(target_hwnd, thickness) {
                Some(geom) => geom,
                None => return,
            };

            let color = state.config.to_colorref();
            let brush = CreateSolidBrush(color);

            // Standard transparent popup without WS_EX_LAYERED so GDI region rendering is immediately visible
            let stroke_hwnd = match CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TRANSPARENT
                    | WS_EX_NOACTIVATE,
                STROKE_CLASS_NAME,
                w!("WintilStrokeOverlay"),
                WS_POPUP,
                ox,
                oy,
                ow,
                oh,
                HWND(std::ptr::null_mut()),
                None,
                None,
                None,
            ) {
                Ok(h) => h,
                Err(_) => {
                    let _ = DeleteObject(brush);
                    return;
                }
            };

            // Set window background brush
            let _ = SetClassLongPtrW(
                stroke_hwnd,
                GCLP_HBRBACKGROUND,
                brush.0 as isize,
            );

            // Create hollow border region: outer rectangle minus inner rectangle
            apply_hollow_region(stroke_hwnd, ow, oh, thickness);

            // Position and display overlay without stealing focus
            let _ = SetWindowPos(
                stroke_hwnd,
                HWND_TOPMOST,
                ox,
                oy,
                ow,
                oh,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(stroke_hwnd, SW_SHOWNOACTIVATE);
            let _ = InvalidateRect(stroke_hwnd, None, true);

            state.overlays.insert(
                hwnd_val,
                OverlayInstance {
                    stroke_hwnd: stroke_hwnd.0 as isize,
                    current_rect: visible_rect,
                    last_brush: brush.0 as isize,
                },
            );
            state.ensure_hook();
        }
    }

    /// Removes a stroke overlay for a window
    pub fn detach_stroke(&self, target_hwnd: HWND) {
        let mut state = self.inner.lock();
        let hwnd_val = target_hwnd.0 as isize;
        if let Some(inst) = state.overlays.remove(&hwnd_val) {
            unsafe {
                let stroke_h = HWND(inst.stroke_hwnd as _);
                let _ = ShowWindow(stroke_h, SW_HIDE);
                let _ = DestroyWindow(stroke_h);
                let _ = DeleteObject(HBRUSH(inst.last_brush as _));
            }
        }
        state.release_hook_if_idle();
    }

    /// Re-renders all active overlays with updated thickness and color
    pub fn update_config(&self, new_config: StrokeConfig) {
        let mut state = self.inner.lock();
        state.config = new_config;

        if !state.config.enabled {
            // Hide all overlays and release system-wide hook immediately
            for inst in state.overlays.values() {
                unsafe {
                    let _ = ShowWindow(HWND(inst.stroke_hwnd as _), SW_HIDE);
                }
            }
            state.release_hook_if_idle();
            return;
        }

        state.ensure_hook();

        let base_thickness = state.config.thickness;
        let color = state.config.to_colorref();

        for (&hwnd_val, inst) in state.overlays.iter_mut() {
            unsafe {
                let target_hwnd = HWND(hwnd_val as _);
                let stroke_h = HWND(inst.stroke_hwnd as _);
                if !IsWindow(target_hwnd).as_bool() {
                    continue;
                }

                let new_brush = CreateSolidBrush(color);
                let _ = SetClassLongPtrW(
                    stroke_h,
                    GCLP_HBRBACKGROUND,
                    new_brush.0 as isize,
                );
                let _ = DeleteObject(HBRUSH(inst.last_brush as _));
                inst.last_brush = new_brush.0 as isize;

                let thickness = get_scaled_thickness(target_hwnd, base_thickness);
                if let Some((ox, oy, ow, oh, visible_rect)) = get_overlay_geometry(target_hwnd, thickness) {
                    inst.current_rect = visible_rect;
                    apply_hollow_region(stroke_h, ow, oh, thickness);

                    let _ = SetWindowPos(
                        stroke_h,
                        HWND_TOPMOST,
                        ox,
                        oy,
                        ow,
                        oh,
                        SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
                    );
                    let _ = ShowWindow(stroke_h, SW_SHOWNOACTIVATE);
                    let _ = InvalidateRect(stroke_h, None, true);
                }
            }
        }
    }

    pub fn get_config(&self) -> StrokeConfig {
        self.inner.lock().config.clone()
    }

    pub fn is_hooked(&self) -> bool {
        self.inner.lock().hook.is_some()
    }

    pub fn overlay_count(&self) -> usize {
        self.inner.lock().overlays.len()
    }

    /// Prunes any overlays whose target HWND no longer exists or is invalid
    pub fn prune_dead(&self) {
        let mut state = self.inner.lock();
        let mut dead_hwnds = Vec::new();
        for (&hwnd_val, inst) in state.overlays.iter() {
            unsafe {
                if !IsWindow(HWND(hwnd_val as _)).as_bool() {
                    let stroke_h = HWND(inst.stroke_hwnd as _);
                    let _ = ShowWindow(stroke_h, SW_HIDE);
                    let _ = DestroyWindow(stroke_h);
                    let _ = DeleteObject(HBRUSH(inst.last_brush as _));
                    dead_hwnds.push(hwnd_val);
                }
            }
        }
        for h in dead_hwnds {
            state.overlays.remove(&h);
        }
        state.release_hook_if_idle();
    }

    /// Returns list of all target window HWNDs currently tracked with an overlay
    pub fn list_overlay_hwnds(&self) -> Vec<isize> {
        self.inner.lock().overlays.keys().copied().collect()
    }
}

impl Drop for StrokeState {
    fn drop(&mut self) {
        unsafe {
            if let Some(h) = self.hook.take() {
                let _ = UnhookWinEvent(HWINEVENTHOOK(h as _));
            }
            for inst in self.overlays.values() {
                let stroke_h = HWND(inst.stroke_hwnd as _);
                let _ = ShowWindow(stroke_h, SW_HIDE);
                let _ = DestroyWindow(stroke_h);
                let _ = DeleteObject(HBRUSH(inst.last_brush as _));
            }
        }
    }
}

unsafe fn apply_hollow_region(stroke_hwnd: HWND, width: i32, height: i32, thickness: i32) {
    let outer_rgn = CreateRectRgn(0, 0, width, height);
    let inner_rgn = CreateRectRgn(
        thickness,
        thickness,
        (width - thickness).max(thickness),
        (height - thickness).max(thickness),
    );
    let combined_rgn = CreateRectRgn(0, 0, 0, 0);

    let _ = CombineRgn(combined_rgn, outer_rgn, inner_rgn, RGN_DIFF);
    let _ = SetWindowRgn(stroke_hwnd, combined_rgn, true);

    let _ = DeleteObject(outer_rgn);
    let _ = DeleteObject(inner_rgn);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stroke_manager_standby_lifecycle() {
        let mgr = StrokeManager::new(StrokeConfig::default());
        assert_eq!(mgr.overlay_count(), 0);
        assert!(!mgr.is_hooked());
        assert!(mgr.list_overlay_hwnds().is_empty());

        // Pruning with no overlays or dead windows should remain clean and on standby
        mgr.prune_dead();
        assert_eq!(mgr.overlay_count(), 0);
        assert!(!mgr.is_hooked());
    }
}
