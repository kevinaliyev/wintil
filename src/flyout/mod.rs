use std::cell::RefCell;
use std::time::Instant;
use windows::core::w;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
    GdiAlphaBlend, GetMonitorInfoW, IntersectClipRect, InvalidateRect, MonitorFromPoint,
    MonitorFromWindow, PtInRect, RestoreDC, RoundRect, SaveDC, ScreenToClient, SelectObject,
    SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    CreateDIBSection, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_RIGHT, DT_SINGLELINE, DT_TOP, DT_VCENTER, DT_WORDBREAK, FW_NORMAL,
    FW_SEMIBOLD, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForSystem, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TrackMouseEvent, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, TME_LEAVE,
    TRACKMOUSEEVENT, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetWindowRect, IsWindowVisible,
    KillTimer, LoadCursorW, RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes,
    SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, IDC_ARROW, LWA_ALPHA,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WA_INACTIVE, WM_ACTIVATE, WM_CHAR, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_PAINT,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS,
    WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub mod layout;

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_DPICHANGED: u32 = 0x02E0;
const HWND_TOPMOST: HWND = HWND(-1 as _);

const TIMER_TOOLTIP_HOVER: usize = 991;
const TOOLTIP_HOVER_DELAY_MS: u32 = 900;

const TIMER_FLYOUT_ANIM: usize = 992;
const ANIM_INTERVAL_MS: u32 = 10;
const OPEN_ANIM_DURATION_MS: u32 = 140;
const CLOSE_ANIM_DURATION_MS: u32 = 90;
const ANIM_SLIDE_OFFSET_DP: i32 = 18;

static LAST_DISMISS_TIME: parking_lot::Mutex<Option<Instant>> = parking_lot::Mutex::new(None);
static FLYOUT_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnimDirection {
    Opening,
    Closing,
}

#[derive(Clone)]
struct AnimState {
    direction: AnimDirection,
    start_time: Instant,
    duration_ms: u32,
    target_x: i32,
    target_y: i32,
    start_y: i32,
    start_alpha: u8,
    target_alpha: u8,
    width: i32,
    height: i32,
    current_alpha: u8,
}

pub const BASE_FLYOUT_WIDTH: i32 = 420;
pub const BASE_FLYOUT_HEIGHT: i32 = 580;

#[derive(Clone, Copy)]
pub struct DpiScale {
    pub dpi: u32,
}

impl DpiScale {
    pub fn for_window(hwnd: HWND) -> Self {
        let dpi = unsafe {
            let d = GetDpiForWindow(hwnd);
            if d == 0 { 96 } else { d }
        };
        Self { dpi }
    }

    #[inline]
    pub fn scale(&self, val: i32) -> i32 {
        ((val as i64 * self.dpi as i64 + 48) / 96) as i32
    }

    #[inline]
    pub fn rect(&self, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left: self.scale(left),
            top: self.scale(top),
            right: self.scale(right),
            bottom: self.scale(bottom),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyoutTab {
    Services,
    Options,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsSubView {
    Keybinds,
    WindowOps,
    Automations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebindModalState {
    pub action_id: String,
    pub action_name: String,
    pub current_bind: String,
    pub recorded_bind: String,
    pub conflict_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlyoutAction {
    Noop,
    SelectTab(FlyoutTab),
    SelectOptionsSubView(OptionsSubView),
    StartEditAutoHideRules,
    CloseAutoHideRulesEditor,
    AddAutoHideRule(String),
    RemoveAutoHideRule(String),
    StartEditKeybinds,
    CloseKeybindsEditor,
    OpenRebindModal {
        action_id: String,
        action_name: String,
        current_bind: String,
    },
    ConfirmRebind,
    ResetRebind,
    ResetDefaultKeybinds,
    RunAutomation(String),
    StartCreateAutomation,
    StartEditAutomation(String),
    SelectAutomation(String),
    RequestDeleteAutomation(Option<String>),
    ConfirmDelete,
    CancelDelete,
    DeleteSelectedAutomation,
    DeleteAutomation(String),
    EditorToggleEnabled,
    EditorFocusName,
    EditorFocusDescription,
    EditorFocusSubserviceName,
    EditorToggleSubservice,
    EditorToggleManualOnly,
    EditorToggleStartup,
    EditorToggleWake,
    EditorToggleInactivity,
    EditorStepInactivity(i64),
    EditorCycleScriptType,
    EditorBrowseScript,
    EditorOpenFolder,
    EditorOpenToml,
    EditorSave,
    EditorCancel,
    EditorDelete,
    UnpinHwnd(isize),
    UnpinAll,
    RestoreHwnd(isize),
    RestoreLast,
    RestoreAll,
    ClearHidden,
    ToggleStroke,
    StepStrokeThickness(i32),
    CycleStrokeColor,
    ToggleHud,
    TestHud,
    OpenConfig,
    ReloadConfig,
    ToggleStartupTask,
    ToggleStartupElevation,
    FlushModifiers,
    ClearLogs,
    OpenLogs,
    ToggleSentryLogging,
    ToggleSentryRespawn,
    ToggleTooltips,
    ToggleKeepGuiOpen,
    RequestExitApp,
    ConfirmExitApp,
    CancelExitApp,
    ExitApp,
}

#[derive(Clone)]
pub(crate) struct ClickTarget {
    pub(crate) rect: RECT,
    pub(crate) action: FlyoutAction,
}

#[derive(Clone)]
pub(crate) struct TooltipHit {
    pub(crate) rect: RECT,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScrollRegionId {
    ServicesSubservices,
    WindowOpsPage,
    WindowOpsPinned,
    WindowOpsHidden,
    WindowOpsRulesPreview,
    AutoHideRulesPage,
    AutoHideRulesActive,
    AutoHideRulesRunning,
    AutomationsList,
    AutomationEditorPage,
    KeybindsOverview,
    KeybindsEditor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutomationTextField {
    Name,
    Description,
    SubserviceName,
}

#[derive(Clone, Default)]
struct ScrollRegistry {
    states: std::collections::HashMap<ScrollRegionId, (layout::ScrollState, RECT)>,
    draw_order: Vec<ScrollRegionId>,
}

struct FlyoutState {
    hwnd: HWND,
    active_tab: FlyoutTab,
    options_subview: OptionsSubView,
    scrolls: ScrollRegistry,
    editing_automation: Option<crate::automations::AutomationEditState>,
    editing_automation_field: Option<AutomationTextField>,
    file_dialog_open: bool,
    editing_autohide_rules: bool,
    editing_keybinds: bool,
    active_rebind: Option<RebindModalState>,
    selected_automation: Option<String>,
    confirming_delete: Option<String>,
    confirming_exit: bool,
    hovered_idx: Option<usize>,
    click_targets: Vec<ClickTarget>,
    tooltips: Vec<TooltipHit>,
    show_tooltips: bool,
    keep_gui_open: bool,
    pending_tooltip: Option<String>,
    visible_tooltip: Option<String>,
    cursor_pos: POINT,
    anim: Option<AnimState>,
}

thread_local! {
    static FLYOUT_STATE: RefCell<Option<FlyoutState>> = const { RefCell::new(None) };
}

pub(crate) fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

pub fn get_target_monitor_info() -> (HMONITOR, MONITORINFO, DpiScale, Option<POINT>) {
    let mut cursor = POINT::default();
    let has_cursor = unsafe { GetCursorPos(&mut cursor).is_ok() };
    let click_pt = if has_cursor { Some(cursor) } else { None };

    let hmon = if has_cursor {
        unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) }
    } else {
        unsafe {
            let primary_pt = POINT { x: 0, y: 0 };
            MonitorFromPoint(primary_pt, MONITOR_DEFAULTTONEAREST)
        }
    };

    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let has_info = unsafe { GetMonitorInfoW(hmon, &mut mi).as_bool() };
    if !has_info {
        let mut wa = RECT::default();
        unsafe {
            let _ = SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut wa as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            );
        }
        mi.rcMonitor = wa;
        mi.rcWork = wa;
    }

    let mut dpi_x: u32 = 0;
    let mut dpi_y: u32 = 0;
    let res = unsafe { GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
    let dpi = if res.is_ok() && dpi_x > 0 {
        dpi_x
    } else {
        let s = unsafe { GetDpiForSystem() };
        if s == 0 { 96 } else { s }
    };

    (hmon, mi, DpiScale { dpi }, click_pt)
}

pub fn get_flyout_geometry(
    mi: &MONITORINFO,
    scale: &DpiScale,
    click_pt: Option<POINT>,
) -> (i32, i32, i32, i32, bool) {
    let width = scale.scale(BASE_FLYOUT_WIDTH);
    let height = scale.scale(BASE_FLYOUT_HEIGHT);
    let margin = scale.scale(12);

    let is_top_bar = mi.rcWork.top > mi.rcMonitor.top;
    let is_bottom_bar = mi.rcWork.bottom < mi.rcMonitor.bottom;
    let is_left_bar = mi.rcWork.left > mi.rcMonitor.left;
    let is_right_bar = mi.rcWork.right < mi.rcMonitor.right;

    let dock_top = if is_top_bar {
        true
    } else if is_bottom_bar {
        false
    } else if let Some(pt) = click_pt {
        pt.y < (mi.rcMonitor.top + mi.rcMonitor.bottom) / 2
    } else {
        false
    };

    let dock_left = if is_left_bar {
        true
    } else if is_right_bar {
        false
    } else if let Some(pt) = click_pt {
        pt.x < (mi.rcMonitor.left + mi.rcMonitor.right) / 2
    } else {
        false
    };

    let x = if dock_left {
        mi.rcWork.left + margin
    } else {
        (mi.rcWork.right - width - margin).max(mi.rcWork.left + margin)
    };

    let y = if dock_top {
        mi.rcWork.top + margin
    } else {
        (mi.rcWork.bottom - height - margin).max(mi.rcWork.top + margin)
    };

    (x, y, width, height, dock_top)
}

pub fn get_flyout_position(width: i32, height: i32, scale: &DpiScale) -> (i32, i32) {
    let (_hmon, mi, _scale, click_pt) = get_target_monitor_info();
    let margin = scale.scale(12);
    let is_top_bar = mi.rcWork.top > mi.rcMonitor.top;
    let is_bottom_bar = mi.rcWork.bottom < mi.rcMonitor.bottom;
    let is_left_bar = mi.rcWork.left > mi.rcMonitor.left;
    let is_right_bar = mi.rcWork.right < mi.rcMonitor.right;

    let dock_top = if is_top_bar {
        true
    } else if is_bottom_bar {
        false
    } else if let Some(pt) = click_pt {
        pt.y < (mi.rcMonitor.top + mi.rcMonitor.bottom) / 2
    } else {
        false
    };

    let dock_left = if is_left_bar {
        true
    } else if is_right_bar {
        false
    } else if let Some(pt) = click_pt {
        pt.x < (mi.rcMonitor.left + mi.rcMonitor.right) / 2
    } else {
        false
    };

    let x = if dock_left {
        mi.rcWork.left + margin
    } else {
        (mi.rcWork.right - width - margin).max(mi.rcWork.left + margin)
    };

    let y = if dock_top {
        mi.rcWork.top + margin
    } else {
        (mi.rcWork.bottom - height - margin).max(mi.rcWork.top + margin)
    };

    (x, y)
}

pub fn is_taskbar_at_top() -> bool {
    let (_hmon, mi, _scale, _pt) = get_target_monitor_info();
    mi.rcWork.top > mi.rcMonitor.top
}

pub fn init_flyout_class() {
    let class_name = w!("WintilFlyoutClass");
    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(flyout_wnd_proc),
        lpszClassName: class_name,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
        ..Default::default()
    };
    unsafe {
        let _ = RegisterClassW(&wnd_class);
    }
}

fn create_flyout_window() -> HWND {
    init_flyout_class();
    let class_name = w!("WintilFlyoutClass");
    let (_hmon, mi, scale, click_pt) = get_target_monitor_info();
    let (x, y, width, height, _) = get_flyout_geometry(&mi, &scale, click_pt);

    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_LAYERED,
            class_name,
            w!("WintilFlyout"),
            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            x,
            y,
            width,
            height,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create flyout window");

        // Windows 11 Rounded corners
        let round_pref = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &round_pref as *const _ as *const _,
            std::mem::size_of_val(&round_pref) as u32,
        );

        // Windows Immersive Dark Mode
        let dark_mode = BOOL(1);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of_val(&dark_mode) as u32,
        );

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        hwnd
    }
}

pub fn record_flyout_dismiss() {
    *LAST_DISMISS_TIME.lock() = Some(Instant::now());
}

pub fn is_flyout_dismiss_recent() -> bool {
    LAST_DISMISS_TIME
        .lock()
        .map(|t| t.elapsed().as_millis() < 350)
        .unwrap_or(false)
}

pub fn is_flyout_closing() -> bool {
    FLYOUT_STATE.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            state
                .anim
                .as_ref()
                .map(|a| a.direction == AnimDirection::Closing)
                .unwrap_or(false)
        } else {
            false
        }
    })
}

pub fn is_flyout_visible() -> bool {
    FLYOUT_STATE.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            unsafe { IsWindowVisible(state.hwnd).as_bool() }
        } else {
            false
        }
    })
}

pub fn get_flyout_hwnd() -> Option<HWND> {
    FLYOUT_STATE.with(|cell| {
        cell.borrow().as_ref().map(|s| s.hwnd)
    })
}

pub fn show_flyout(tab: Option<FlyoutTab>) {
    let mut show_cmd = None;

    FLYOUT_STATE.with(|cell| {
        let mut state_opt = cell.borrow_mut();
        if state_opt.is_none() {
            let hwnd = create_flyout_window();
            FLYOUT_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
            *state_opt = Some(FlyoutState {
                hwnd,
                active_tab: tab.unwrap_or(FlyoutTab::Services),
                options_subview: OptionsSubView::Keybinds,
                scrolls: ScrollRegistry::default(),
                editing_automation: None,
                editing_automation_field: None,
                file_dialog_open: false,
                editing_autohide_rules: false,
                editing_keybinds: false,
                active_rebind: None,
                selected_automation: None,
                confirming_delete: None,
                confirming_exit: false,
                hovered_idx: None,
                click_targets: Vec::new(),
                tooltips: Vec::new(),
                show_tooltips: false,
                keep_gui_open: false,
                pending_tooltip: None,
                visible_tooltip: None,
                cursor_pos: POINT::default(),
                anim: None,
            });
        }

        if let Some(state) = state_opt.as_mut() {
            if let Some(t) = tab {
                state.active_tab = t;
            }

            let (_hmon, mi, scale, click_pt) = get_target_monitor_info();
            let (target_x, target_y, width, height, is_top) =
                get_flyout_geometry(&mi, &scale, click_pt);
            let slide_offset = scale.scale(ANIM_SLIDE_OFFSET_DP);
            let start_y = if is_top {
                target_y - slide_offset
            } else {
                target_y + slide_offset
            };

            // Clear tooltips on show
            state.pending_tooltip = None;
            state.visible_tooltip = None;
            state.hovered_idx = None;

            let anim = AnimState {
                direction: AnimDirection::Opening,
                start_time: Instant::now(),
                duration_ms: OPEN_ANIM_DURATION_MS,
                target_x,
                target_y,
                start_y,
                start_alpha: 25,
                target_alpha: 255,
                width,
                height,
                current_alpha: 25,
            };
            state.anim = Some(anim);
            let was_visible = unsafe { IsWindowVisible(state.hwnd).as_bool() };
            if !was_visible {
                crate::sentry::on_gui_opened();
            }
            show_cmd = Some((state.hwnd, target_x, start_y, width, height));
        }
    });

    if let Some((hwnd, target_x, start_y, width, height)) = show_cmd {
        unsafe {
            let _ = KillTimer(hwnd, TIMER_FLYOUT_ANIM);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 25, LWA_ALPHA);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                target_x,
                start_y,
                width,
                height,
                SWP_SHOWWINDOW,
            );
            let _ = SetForegroundWindow(hwnd);
            let _ = InvalidateRect(hwnd, None, false);
            let _ = SetTimer(hwnd, TIMER_FLYOUT_ANIM, ANIM_INTERVAL_MS, None);
        }
    }
}

pub fn hide_flyout() {
    record_flyout_dismiss();

    let mut start_timer_hwnd = None;
    let mut should_resume_hotkeys = false;

    FLYOUT_STATE.with(|cell| {
        if let Ok(mut state_opt) = cell.try_borrow_mut() {
            if let Some(state) = state_opt.as_mut() {
                state.pending_tooltip = None;
                state.visible_tooltip = None;
                state.hovered_idx = None;
                state.confirming_delete = None;
                state.confirming_exit = false;
                if state.active_rebind.take().is_some() {
                    should_resume_hotkeys = true;
                }

                let is_visible = unsafe { IsWindowVisible(state.hwnd).as_bool() };
                if !is_visible {
                    state.anim = None;
                    return;
                }

                if let Some(ref a) = state.anim {
                    if a.direction == AnimDirection::Closing {
                        return;
                    }
                }

                // In-place closing: capture CURRENT window rectangle so UI never jumps!
                let mut current_rc = RECT::default();
                unsafe {
                    let _ = GetWindowRect(state.hwnd, &mut current_rc);
                }
                let curr_x = current_rc.left;
                let curr_y = current_rc.top;
                let width = (current_rc.right - current_rc.left).max(100);
                let height = (current_rc.bottom - current_rc.top).max(100);

                let hmon = unsafe { MonitorFromWindow(state.hwnd, MONITOR_DEFAULTTONEAREST) };
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let is_top = if unsafe { GetMonitorInfoW(hmon, &mut mi).as_bool() } {
                    curr_y < (mi.rcMonitor.top + mi.rcMonitor.bottom) / 2
                } else {
                    false
                };

                let slide_offset = 12;
                let close_target_y = if is_top {
                    curr_y - slide_offset
                } else {
                    curr_y + slide_offset
                };

                let current_alpha = state.anim.as_ref().map(|a| a.current_alpha).unwrap_or(255);

                let anim = AnimState {
                    direction: AnimDirection::Closing,
                    start_time: Instant::now(),
                    duration_ms: CLOSE_ANIM_DURATION_MS,
                    target_x: curr_x,
                    target_y: close_target_y,
                    start_y: curr_y,
                    start_alpha: current_alpha,
                    target_alpha: 0,
                    width,
                    height,
                    current_alpha,
                };
                state.anim = Some(anim);
                start_timer_hwnd = Some(state.hwnd);
            }
        }
    });

    if should_resume_hotkeys {
        crate::app_resume_hotkeys();
    }

    if let Some(hwnd) = start_timer_hwnd {
        unsafe {
            let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
            let _ = KillTimer(hwnd, TIMER_FLYOUT_ANIM);
            let _ = SetTimer(hwnd, TIMER_FLYOUT_ANIM, ANIM_INTERVAL_MS, None);
        }
    }
}

pub fn hide_flyout_immediate() {
    record_flyout_dismiss();
    let mut hide_hwnd = None;
    let mut should_resume_hotkeys = false;
    FLYOUT_STATE.with(|cell| {
        if let Ok(mut state_opt) = cell.try_borrow_mut() {
            if let Some(state) = state_opt.as_mut() {
                state.anim = None;
                state.pending_tooltip = None;
                state.visible_tooltip = None;
                state.hovered_idx = None;
                state.confirming_delete = None;
                state.confirming_exit = false;
                if state.active_rebind.take().is_some() {
                    should_resume_hotkeys = true;
                }
                hide_hwnd = Some(state.hwnd);
            }
        }
    });

    if should_resume_hotkeys {
        crate::app_resume_hotkeys();
    }

    if let Some(hwnd) = hide_hwnd {
        unsafe {
            let _ = KillTimer(hwnd, TIMER_FLYOUT_ANIM);
            let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        }
    }
}

pub fn toggle_flyout() {
    let action = FLYOUT_STATE.with(|cell| {
        if let Ok(state_opt) = cell.try_borrow() {
            if let Some(state) = state_opt.as_ref() {
                let is_visible = unsafe { IsWindowVisible(state.hwnd).as_bool() };
                let is_closing = state
                    .anim
                    .as_ref()
                    .map(|a| a.direction == AnimDirection::Closing)
                    .unwrap_or(false);

                if is_visible && !is_closing {
                    return 1; // hide
                }

                if is_closing || is_flyout_dismiss_recent() {
                    // User clicked tray icon to dismiss the menu: stay closed, do not rehydrate!
                    return 0; // ignore
                }

                2 // show
            } else {
                2 // show
            }
        } else {
            0
        }
    });

    match action {
        1 => hide_flyout(),
        2 => show_flyout(None),
        _ => {}
    }
}

pub fn invalidate_flyout() {
    let raw = FLYOUT_HWND.load(std::sync::atomic::Ordering::Relaxed);
    if raw != 0 {
        let hwnd = HWND(raw as *mut std::ffi::c_void);
        unsafe {
            let _ = InvalidateRect(hwnd, None, false);
        }
    }
}

unsafe extern "system" fn flyout_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ACTIVATE => {
            let active = (wparam.0 & 0xffff) as u32;
            if active == WA_INACTIVE {
                let is_visible = unsafe { IsWindowVisible(hwnd).as_bool() };
                let suppress = FLYOUT_STATE.with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .map(|s| s.keep_gui_open || s.file_dialog_open)
                        .unwrap_or(false)
                });
                if is_visible && !suppress {
                    hide_flyout();
                }
            }
            LRESULT(0)
        }

        WM_TIMER => {
            if wparam.0 == TIMER_FLYOUT_ANIM {
                let mut kill_timer = false;
                let mut hide_window = false;
                let mut new_pos = None;
                let mut new_alpha = 255u8;

                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        if let Some(ref mut anim) = state.anim {
                            let elapsed = anim.start_time.elapsed().as_millis() as f32;
                            let t = (elapsed / anim.duration_ms as f32).clamp(0.0, 1.0);

                            match anim.direction {
                                AnimDirection::Opening => {
                                    let progress = 1.0 - (1.0 - t).powi(3);
                                    let curr_y = (anim.start_y as f32 + (anim.target_y - anim.start_y) as f32 * progress).round() as i32;
                                    let curr_alpha = (anim.start_alpha as f32 + (anim.target_alpha as f32 - anim.start_alpha as f32) * progress).round() as u8;

                                    anim.current_alpha = curr_alpha;
                                    new_pos = Some((anim.target_x, curr_y, anim.width, anim.height));
                                    new_alpha = curr_alpha;

                                    if t >= 1.0 {
                                        kill_timer = true;
                                        new_pos = Some((anim.target_x, anim.target_y, anim.width, anim.height));
                                        new_alpha = 255;
                                        state.anim = None;
                                    }
                                }
                                AnimDirection::Closing => {
                                    let progress = t * t;
                                    let curr_y = (anim.start_y as f32 + (anim.target_y - anim.start_y) as f32 * progress).round() as i32;
                                    let curr_alpha = (anim.start_alpha as f32 + (anim.target_alpha as f32 - anim.start_alpha as f32) * progress).round() as u8;

                                    anim.current_alpha = curr_alpha;
                                    new_pos = Some((anim.target_x, curr_y, anim.width, anim.height));
                                    new_alpha = curr_alpha;

                                    if t >= 1.0 {
                                        kill_timer = true;
                                        hide_window = true;
                                        state.anim = None;
                                    }
                                }
                            }
                        } else {
                            kill_timer = true;
                        }
                    }
                });

                unsafe {
                    if let Some((x, y, w, h)) = new_pos {
                        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), new_alpha, LWA_ALPHA);
                        let _ = SetWindowPos(
                            hwnd,
                            HWND_TOPMOST,
                            x,
                            y,
                            w,
                            h,
                            SWP_NOACTIVATE | SWP_NOZORDER,
                        );
                    }
                    if hide_window {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                    }
                    if kill_timer {
                        let _ = KillTimer(hwnd, TIMER_FLYOUT_ANIM);
                    }
                }
            } else if wparam.0 == TIMER_TOOLTIP_HOVER {
                unsafe {
                    let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
                }
                let mut should_draw = false;
                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        if state.pending_tooltip.is_some() && state.visible_tooltip != state.pending_tooltip {
                            state.visible_tooltip = state.pending_tooltip.clone();
                            should_draw = true;
                        }
                    }
                });
                if should_draw {
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }
            }
            LRESULT(0)
        }

        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xffff) as u32;
            let scale = DpiScale { dpi: if new_dpi > 0 { new_dpi } else { 96 } };
            let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !unsafe { GetMonitorInfoW(hmon, &mut mi).as_bool() } {
                let mut wa = RECT::default();
                unsafe {
                    let _ = SystemParametersInfoW(
                        SPI_GETWORKAREA,
                        0,
                        Some(&mut wa as *mut _ as *mut _),
                        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    );
                }
                mi.rcMonitor = wa;
                mi.rcWork = wa;
            }

            let mut current_rc = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut current_rc);
            }
            let click_pt = Some(POINT { x: current_rc.left, y: current_rc.top });
            let (target_x, target_y, width, height, _) = get_flyout_geometry(&mi, &scale, click_pt);

            let is_visible = unsafe { IsWindowVisible(hwnd).as_bool() };
            if is_visible {
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        target_x,
                        target_y,
                        width,
                        height,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    );
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }

            FLYOUT_STATE.with(|cell| {
                if let Ok(mut state_opt) = cell.try_borrow_mut() {
                    if let Some(state) = state_opt.as_mut() {
                        if let Some(ref mut anim) = state.anim {
                            anim.width = width;
                            anim.height = height;
                            anim.target_x = target_x;
                            anim.target_y = target_y;
                        }
                    }
                }
            });
            LRESULT(0)
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = VIRTUAL_KEY(wparam.0 as u16);
            let mut resume_hk = false;
            let mut commit_rebind: Option<(String, String)> = None;

            let handled = FLYOUT_STATE.with(|cell| {
                if let Ok(mut state_opt) = cell.try_borrow_mut() {
                    if let Some(state) = state_opt.as_mut() {
                        if state.editing_automation.is_some() && state.editing_automation_field.is_some() {
                            if vk == VK_RETURN || vk == VK_ESCAPE {
                                state.editing_automation_field = None;
                                return true;
                            }
                            if vk == VIRTUAL_KEY(0x08) {
                                let active_field = state.editing_automation_field;
                                if let Some(ref mut ed) = state.editing_automation {
                                    match active_field {
                                        Some(AutomationTextField::Name) => { ed.name.pop(); }
                                        Some(AutomationTextField::Description) => { ed.description.pop(); }
                                        Some(AutomationTextField::SubserviceName) => { ed.subservice_name.pop(); }
                                        None => {}
                                    }
                                }
                                return true;
                            }
                            return false;
                        }
                        if state.active_rebind.is_some() {
                            if vk == VK_ESCAPE {
                                state.active_rebind = None;
                                resume_hk = true;
                                return true;
                            }
                            if vk == VK_RETURN {
                                let can_commit = state
                                    .active_rebind
                                    .as_ref()
                                    .map(|r| {
                                        r.conflict_error.is_none()
                                            && crate::config::keys::parse_keybind(&r.recorded_bind).is_some()
                                            && crate::app_check_hotkey_conflict(&r.action_id, &r.recorded_bind).is_none()
                                    })
                                    .unwrap_or(false);
                                if can_commit {
                                    if let Some(rebind) = state.active_rebind.take() {
                                        commit_rebind = Some((rebind.action_id, rebind.recorded_bind));
                                    }
                                }
                                return true;
                            }

                            // Disallow mouse keys (left, right, middle, xbuttons)
                            if crate::config::keys::is_mouse_virtual_key(vk) {
                                return true;
                            }

                            // Inspect modifier keys
                            let mut mods = 0;
                            unsafe {
                                if (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
                                    mods |= MOD_ALT.0;
                                }
                                if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
                                    mods |= MOD_CONTROL.0;
                                }
                                if (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
                                    mods |= MOD_SHIFT.0;
                                }
                                if (GetKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
                                    || (GetKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
                                {
                                    mods |= MOD_WIN.0;
                                }
                            }

                            let is_pure_mod = matches!(
                                vk,
                                VK_SHIFT
                                    | VK_LSHIFT
                                    | VK_RSHIFT
                                    | VK_CONTROL
                                    | VK_LCONTROL
                                    | VK_RCONTROL
                                    | VK_MENU
                                    | VK_LMENU
                                    | VK_RMENU
                                    | VK_LWIN
                                    | VK_RWIN
                            );

                            if let Some(ref mut rebind) = state.active_rebind {
                                if is_pure_mod {
                                    let mut m = Vec::new();
                                    if mods & MOD_ALT.0 != 0 { m.push("Alt"); }
                                    if mods & MOD_CONTROL.0 != 0 { m.push("Ctrl"); }
                                    if mods & MOD_SHIFT.0 != 0 { m.push("Shift"); }
                                    if mods & MOD_WIN.0 != 0 { m.push("Win"); }
                                    if !m.is_empty() {
                                        rebind.recorded_bind = format!("{} + ...", m.join(" + "));
                                        rebind.conflict_error = None;
                                    }
                                } else if let Some(combo) = crate::config::keys::format_hotkey(mods, vk) {
                                    let conflict = crate::app_check_hotkey_conflict(&rebind.action_id, &combo);
                                    rebind.recorded_bind = combo;
                                    rebind.conflict_error = conflict.map(|name| {
                                        format!("Conflict: Already bound to '{}'", name)
                                    });
                                }
                            }
                            return true;
                        }

                        if vk == VK_ESCAPE {
                            if state.confirming_exit {
                                state.confirming_exit = false;
                                return true;
                            }
                            if state.confirming_delete.is_some() {
                                state.confirming_delete = None;
                                return true;
                            }
                            if state.editing_automation.is_some() {
                                state.editing_automation = None;
                                return true;
                            }
                            if state.editing_autohide_rules {
                                state.editing_autohide_rules = false;
                                return true;
                            }
                            if state.editing_keybinds {
                                state.editing_keybinds = false;
                                return true;
                            }
                        }
                    }
                }
                false
            });

            if resume_hk {
                crate::app_resume_hotkeys();
            }
            if let Some((action_id, recorded_bind)) = commit_rebind {
                crate::app_update_hotkey(&action_id, &recorded_bind);
            }

            if handled {
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            } else if vk == VK_ESCAPE {
                hide_flyout();
            }
            LRESULT(0)
        }

        WM_CHAR => {
            let ch_code = wparam.0 as u32;
            let mut handled = false;
            if ch_code >= 32 && ch_code != 127 {
                if let Some(c) = char::from_u32(ch_code) {
                    FLYOUT_STATE.with(|cell| {
                        if let Ok(mut state_opt) = cell.try_borrow_mut() {
                            if let Some(state) = state_opt.as_mut() {
                                if let Some(field) = state.editing_automation_field {
                                    if let Some(ref mut ed) = state.editing_automation {
                                        let (target_str, max_len) = match field {
                                            AutomationTextField::Name => (&mut ed.name, 48),
                                            AutomationTextField::Description => (&mut ed.description, 120),
                                            AutomationTextField::SubserviceName => (&mut ed.subservice_name, 48),
                                        };
                                        if target_str.chars().count() < max_len {
                                            target_str.push(c);
                                            handled = true;
                                        }
                                    }
                                }
                            }
                        }
                    });
                }
            }
            if handled {
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        WM_KEYUP | WM_SYSKEYUP => {
            let rebind_active = FLYOUT_STATE.with(|cell| {
                if let Ok(state_opt) = cell.try_borrow() {
                    state_opt.as_ref().map(|s| s.active_rebind.is_some()).unwrap_or(false)
                } else {
                    false
                }
            });
            if rebind_active {
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        WM_MOUSEWHEEL | windows::Win32::UI::WindowsAndMessaging::WM_MOUSEHWHEEL => {
            let rebind_active = FLYOUT_STATE.with(|cell| {
                if let Ok(state_opt) = cell.try_borrow() {
                    state_opt.as_ref().map(|s| s.active_rebind.is_some()).unwrap_or(false)
                } else {
                    false
                }
            });
            if rebind_active {
                return LRESULT(0);
            }
            if msg == WM_MOUSEWHEEL {
                let wheel_delta = ((wparam.0 >> 16) & 0xffff) as i16;
                let mut pt = POINT::default();
                let scale = DpiScale::for_window(hwnd);
                let step_px = scale.scale(28);
                let scrolled = unsafe {
                    let _ = GetCursorPos(&mut pt);
                    let _ = ScreenToClient(hwnd, &mut pt);
                    FLYOUT_STATE.with(|cell| {
                        if let Ok(mut state_opt) = cell.try_borrow_mut() {
                            if let Some(state) = state_opt.as_mut() {
                                let order = state.scrolls.draw_order.clone();
                                for id in order.into_iter().rev() {
                                    if let Some((scroll_st, rect)) = state.scrolls.states.get_mut(&id) {
                                        if PtInRect(rect, pt).as_bool() && scroll_st.is_scrollable() {
                                            let prev = scroll_st.scroll_offset;
                                            scroll_st.apply_wheel_delta(wheel_delta, step_px);
                                            if scroll_st.scroll_offset != prev {
                                                return true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        false
                    })
                };
                if scrolled {
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                    return LRESULT(0);
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        WM_ERASEBKGND => LRESULT(1),

        WM_PAINT => {
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint_flyout(hwnd, hdc);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let pt = POINT { x, y };

            let mut hovered_target = None;
            let mut hovered_tip = None;

            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    for (i, t) in state.click_targets.iter().enumerate().rev() {
                        if PtInRect(&t.rect, pt).as_bool() {
                            hovered_target = Some(i);
                            break;
                        }
                    }
                    if state.show_tooltips {
                        for tip in state.tooltips.iter().rev() {
                            if PtInRect(&tip.rect, pt).as_bool() {
                                hovered_tip = Some(tip.text.clone());
                                break;
                            }
                        }
                    }
                }
            });

            let mut invalidate = false;
            let mut start_timer = false;
            let mut stop_timer = false;

            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.cursor_pos = pt;
                    if state.hovered_idx != hovered_target {
                        state.hovered_idx = hovered_target;
                        invalidate = true;
                    }
                    if state.pending_tooltip != hovered_tip {
                        state.pending_tooltip = hovered_tip.clone();
                        if state.visible_tooltip.is_some() {
                            state.visible_tooltip = None;
                            invalidate = true;
                        }
                        if hovered_tip.is_some() {
                            start_timer = true;
                        } else {
                            stop_timer = true;
                        }
                    }
                }
            });

            if stop_timer {
                unsafe {
                    let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
                }
            } else if start_timer {
                unsafe {
                    let _ = SetTimer(hwnd, TIMER_TOOLTIP_HOVER, TOOLTIP_HOVER_DELAY_MS, None);
                }
            }

            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
                if invalidate {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }

            LRESULT(0)
        }

        WM_MOUSELEAVE => {
            unsafe {
                let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
            }
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    let had_hover = state.hovered_idx.is_some()
                        || state.pending_tooltip.is_some()
                        || state.visible_tooltip.is_some();
                    state.hovered_idx = None;
                    state.pending_tooltip = None;
                    state.visible_tooltip = None;
                    if had_hover {
                        unsafe {
                            let _ = InvalidateRect(hwnd, None, false);
                        }
                    }
                }
            });
            LRESULT(0)
        }

        WM_LBUTTONDOWN => LRESULT(0),

        WM_LBUTTONUP => {
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let pt = POINT { x, y };

            let mut matched_action = None;
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    for t in state.click_targets.iter().rev() {
                        if PtInRect(&t.rect, pt).as_bool() {
                            matched_action = Some(t.action.clone());
                            break;
                        }
                    }
                }
            });

            if let Some(action) = matched_action {
                execute_flyout_action(hwnd, action);
            }

            LRESULT(0)
        }

        WM_DESTROY => LRESULT(0),

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn execute_flyout_action(hwnd: HWND, action: FlyoutAction) {
    unsafe {
        let _ = KillTimer(hwnd, TIMER_TOOLTIP_HOVER);
    }
    FLYOUT_STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.hovered_idx = None;
            state.pending_tooltip = None;
            state.visible_tooltip = None;
            if !matches!(
                action,
                FlyoutAction::EditorFocusName
                    | FlyoutAction::EditorFocusDescription
                    | FlyoutAction::EditorFocusSubserviceName
            ) {
                state.editing_automation_field = None;
            }
        }
    });

    match action {
        FlyoutAction::Noop => {}

        FlyoutAction::SelectTab(tab) => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.active_tab = tab;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::SelectOptionsSubView(subview) => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.options_subview = subview;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::StartEditAutoHideRules => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_autohide_rules = true;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::CloseAutoHideRulesEditor => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_autohide_rules = false;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::AddAutoHideRule(exe) => {
            crate::app_add_auto_hide_rule(exe);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RemoveAutoHideRule(exe) => {
            crate::app_remove_auto_hide_rule(&exe);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::StartEditKeybinds => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_keybinds = true;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::CloseKeybindsEditor => {
            let mut should_resume = false;
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_keybinds = false;
                    if state.active_rebind.take().is_some() {
                        should_resume = true;
                    }
                }
            });
            if should_resume {
                crate::app_resume_hotkeys();
            }
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::OpenRebindModal { action_id, action_name, current_bind } => {
            crate::app_suspend_hotkeys();
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.active_rebind = Some(RebindModalState {
                        action_id,
                        action_name,
                        recorded_bind: current_bind.clone(),
                        current_bind,
                        conflict_error: None,
                    });
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ConfirmRebind => {
            let rebind_opt = FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    let can_commit = state
                        .active_rebind
                        .as_ref()
                        .map(|r| {
                            r.conflict_error.is_none()
                                && crate::config::keys::parse_keybind(&r.recorded_bind).is_some()
                                && crate::app_check_hotkey_conflict(&r.action_id, &r.recorded_bind).is_none()
                        })
                        .unwrap_or(false);
                    if can_commit {
                        state.active_rebind.take()
                    } else {
                        if let Some(ref mut r) = state.active_rebind {
                            if let Some(conflict_name) = crate::app_check_hotkey_conflict(&r.action_id, &r.recorded_bind) {
                                r.conflict_error = Some(format!("Conflict: Already bound to '{}'", conflict_name));
                            }
                        }
                        None
                    }
                } else {
                    None
                }
            });
            if let Some(rebind) = rebind_opt {
                crate::app_update_hotkey(&rebind.action_id, &rebind.recorded_bind);
            }
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ResetRebind => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.active_rebind = None;
                }
            });
            crate::app_resume_hotkeys();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ResetDefaultKeybinds => {
            crate::app_reset_default_hotkeys();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RunAutomation(name) => {
            crate::app_run_automation(&name);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::StartCreateAutomation => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_automation = Some(crate::automations::AutomationEditState::default());
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::StartEditAutomation(name) => {
            if let Some(edit) = crate::app_get_automation_for_edit(&name) {
                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.editing_automation = Some(edit);
                    }
                });
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
        }

        FlyoutAction::SelectAutomation(name) => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.selected_automation = Some(name);
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RequestDeleteAutomation(target_opt) => {
            let to_confirm = if let Some(n) = target_opt {
                Some(n)
            } else {
                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow().as_ref() {
                        if let Some(ref sel) = state.selected_automation {
                            Some(sel.clone())
                        } else {
                            crate::app_get_flyout_snapshot()
                                .and_then(|snap| snap.automations.first().map(|a| a.name.clone()))
                        }
                    } else {
                        None
                    }
                })
            };

            if let Some(name) = to_confirm {
                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.selected_automation = Some(name.clone());
                        state.confirming_delete = Some(name);
                    }
                });
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
        }

        FlyoutAction::ConfirmDelete => {
            let to_delete = FLYOUT_STATE.with(|cell| {
                let mut state_opt = cell.borrow_mut();
                if let Some(state) = state_opt.as_mut() {
                    let name = state.confirming_delete.take();
                    if let Some(ref n) = name {
                        if state.selected_automation.as_deref() == Some(n) {
                            state.selected_automation = None;
                        }
                        if let Some(ref ed) = state.editing_automation {
                            if ed.name == *n || ed.original_name == *n {
                                state.editing_automation = None;
                            }
                        }
                    }
                    name
                } else {
                    None
                }
            });
            if let Some(name) = to_delete {
                let _ = crate::app_delete_automation(&name);
            }
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::CancelDelete => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.confirming_delete = None;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::DeleteSelectedAutomation => {
            execute_flyout_action(hwnd, FlyoutAction::RequestDeleteAutomation(None));
        }

        FlyoutAction::DeleteAutomation(name) => {
            execute_flyout_action(hwnd, FlyoutAction::RequestDeleteAutomation(Some(name)));
        }

        FlyoutAction::EditorToggleEnabled => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.enabled = !ed.enabled;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorFocusName => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.editing_automation.is_some() {
                        state.editing_automation_field = Some(AutomationTextField::Name);
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorFocusDescription => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.editing_automation.is_some() {
                        state.editing_automation_field = Some(AutomationTextField::Description);
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorToggleManualOnly => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.only_manual = !ed.only_manual;
                        if ed.only_manual {
                            ed.on_startup = false;
                            ed.on_wake = false;
                            ed.on_inactivity_resume = false;
                        }
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorFocusSubserviceName => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if state.editing_automation.is_some() {
                        state.editing_automation_field = Some(AutomationTextField::SubserviceName);
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorToggleSubservice => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.is_subservice = !ed.is_subservice;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorToggleStartup => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.on_startup = !ed.on_startup;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorToggleWake => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.on_wake = !ed.on_wake;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorToggleInactivity => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        ed.on_inactivity_resume = !ed.on_inactivity_resume;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorStepInactivity(delta) => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        let cur = ed.inactivity_threshold_secs as i64;
                        let next = (cur + delta).clamp(15, 3600);
                        ed.inactivity_threshold_secs = next as u64;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorCycleScriptType => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    if let Some(ref mut ed) = state.editing_automation {
                        let cur = &ed.script;
                        ed.script = if cur.ends_with(".ps1") {
                            "task.bat".to_string()
                        } else if cur.ends_with(".bat") {
                            "task.cmd".to_string()
                        } else {
                            "task.ps1".to_string()
                        };
                        ed.source_script_path = None;
                    }
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorBrowseScript => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.file_dialog_open = true;
                }
            });
            let picked = unsafe { browse_script_file(hwnd) };
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.file_dialog_open = false;
                    if let Some(ref path) = picked {
                        if let Some(ref mut ed) = state.editing_automation {
                            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                                ed.script = fname.to_string();
                                ed.source_script_path = Some(path.clone());
                            }
                        }
                    }
                }
            });
            unsafe {
                let _ = SetForegroundWindow(hwnd);
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorOpenFolder => {
            let dir = FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    if let Some(ref ed) = state.editing_automation {
                        if ed.dir_path.as_os_str().is_empty() {
                            crate::config::get_config_dir().join("automations")
                        } else {
                            ed.dir_path.clone()
                        }
                    } else {
                        crate::config::get_config_dir().join("automations")
                    }
                } else {
                    crate::config::get_config_dir().join("automations")
                }
            });
            crate::app_open_automation_folder(&dir);
        }

        FlyoutAction::EditorOpenToml => {
            let manifest = FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow().as_ref() {
                    if let Some(ref ed) = state.editing_automation {
                        if ed.dir_path.as_os_str().is_empty() {
                            crate::config::get_config_dir().join("automations").join(&ed.name).join("automation.toml")
                        } else {
                            ed.dir_path.join("automation.toml")
                        }
                    } else {
                        crate::config::get_config_dir().join("automations").join("automation.toml")
                    }
                } else {
                    crate::config::get_config_dir().join("automations").join("automation.toml")
                }
            });
            crate::app_open_automation_toml(&manifest);
        }

        FlyoutAction::EditorSave => {
            let conflict = FLYOUT_STATE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|s| s.editing_automation.as_ref())
                    .and_then(crate::app_check_automation_conflict)
            });
            if conflict.is_some() {
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
                return;
            }
            let ed_opt = FLYOUT_STATE.with(|cell| {
                let mut state_opt = cell.borrow_mut();
                if let Some(state) = state_opt.as_mut() {
                    state.editing_automation.take()
                } else {
                    None
                }
            });
            if let Some(ed) = ed_opt {
                let _ = crate::app_save_automation(&ed);
            }
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorCancel => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.editing_automation = None;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::EditorDelete => {
            let target_name = FLYOUT_STATE.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .and_then(|s| s.editing_automation.as_ref().map(|ed| ed.name.clone()))
            });
            if let Some(name) = target_name {
                FLYOUT_STATE.with(|cell| {
                    if let Some(state) = cell.borrow_mut().as_mut() {
                        state.confirming_delete = Some(name);
                    }
                });
                unsafe {
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
        }

        FlyoutAction::UnpinHwnd(h) => {
            crate::app_unpin_hwnd(h);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::UnpinAll => {
            crate::app_unpin_all();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RestoreHwnd(h) => {
            crate::app_restore_hwnd(h);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RestoreLast => {
            crate::app_restore_last();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RestoreAll => {
            crate::app_restore_all();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ClearHidden => {
            crate::app_clear_hidden();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleStroke => {
            crate::app_toggle_stroke();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::StepStrokeThickness(delta) => {
            crate::app_step_stroke_thickness(delta);
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::CycleStrokeColor => {
            crate::app_cycle_stroke_color();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleHud => {
            crate::app_toggle_hud();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::TestHud => {
            crate::app_test_hud();
        }

        FlyoutAction::OpenConfig => {
            crate::app_open_config();
        }

        FlyoutAction::ReloadConfig => {
            crate::app_reload_config();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleStartupTask => {
            crate::startup::toggle_task();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleStartupElevation => {
            crate::startup::toggle_elevation();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::FlushModifiers => {
            crate::app_flush_modifiers();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ClearLogs => {
            crate::app_clear_logs();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::OpenLogs => {
            crate::app_open_logs();
        }

        FlyoutAction::ToggleSentryLogging => {
            crate::app_toggle_sentry_logging();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleSentryRespawn => {
            crate::app_toggle_sentry_respawn();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleTooltips => {
            crate::app_toggle_tooltips();
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ToggleKeepGuiOpen => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.keep_gui_open = !state.keep_gui_open;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::RequestExitApp => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.confirming_exit = true;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::CancelExitApp => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.confirming_exit = false;
                }
            });
            unsafe {
                let _ = InvalidateRect(hwnd, None, false);
            }
        }

        FlyoutAction::ConfirmExitApp | FlyoutAction::ExitApp => {
            FLYOUT_STATE.with(|cell| {
                if let Some(state) = cell.borrow_mut().as_mut() {
                    state.confirming_exit = false;
                }
            });
            hide_flyout_immediate();
            crate::app_exit();
        }
    }
}

fn paint_flyout(hwnd: HWND, hdc: windows::Win32::Graphics::Gdi::HDC) {
    let snapshot = match crate::app_get_flyout_snapshot() {
        Some(s) => s,
        None => return,
    };

    let scale = DpiScale::for_window(hwnd);

    let (
        active_tab,
        options_subview,
        mut scrolls,
        editing_automation,
        editing_automation_field,
        editing_autohide_rules,
        editing_keybinds,
        active_rebind,
        selected_automation,
        confirming_delete,
        confirming_exit,
        hovered_idx,
        active_tooltip,
        cursor_pos,
        keep_gui_open,
    ) = FLYOUT_STATE.with(|cell| {
        if let Some(state) = cell.borrow().as_ref() {
            (
                state.active_tab,
                state.options_subview,
                ScrollRegistry {
                    states: state.scrolls.states.clone(),
                    draw_order: Vec::new(),
                },
                state.editing_automation.clone(),
                state.editing_automation_field,
                state.editing_autohide_rules,
                state.editing_keybinds,
                state.active_rebind.clone(),
                state.selected_automation.clone(),
                state.confirming_delete.clone(),
                state.confirming_exit,
                state.hovered_idx,
                state.visible_tooltip.clone(),
                state.cursor_pos,
                state.keep_gui_open,
            )
        } else {
            (
                FlyoutTab::Services,
                OptionsSubView::Keybinds,
                ScrollRegistry::default(),
                None,
                None,
                false,
                false,
                None,
                None,
                None,
                false,
                None,
                None,
                POINT::default(),
                false,
            )
        }
    });

    let mut new_targets = Vec::new();
    let mut new_tooltips = Vec::new();

    let mut client_rc = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client_rc);
    }
    let win_w = client_rc.right - client_rc.left;
    let win_h = client_rc.bottom - client_rc.top;

    unsafe {
        let mem_dc = CreateCompatibleDC(hdc);
        let mem_bmp = CreateCompatibleBitmap(hdc, win_w, win_h);
        let old_bmp = SelectObject(mem_dc, mem_bmp);

        SetBkMode(mem_dc, TRANSPARENT);

        // Fonts: DPI scaled, compact and crisp weights
        let font_small = CreateFontW(
            -scale.scale(10),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            w!("Segoe UI"),
        );
        let font_normal = CreateFontW(
            -scale.scale(12),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            w!("Segoe UI"),
        );
        let font_semibold = CreateFontW(
            -scale.scale(12),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            w!("Segoe UI"),
        );
        let font_title = CreateFontW(
            -scale.scale(14),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(mem_dc, font_normal);

        // 1. Background fill
        let bg_brush = CreateSolidBrush(rgb(26, 26, 30));
        FillRect(mem_dc, &client_rc, bg_brush);
        let _ = DeleteObject(bg_brush);

        // Border outline
        let border_brush = CreateSolidBrush(rgb(56, 56, 64));
        FrameRect(mem_dc, &client_rc, border_brush);
        let _ = DeleteObject(border_brush);

        // 2. Either Full-GUI Automation Editor OR Auto-Hide Rules Editor OR Tab Navigation & Tab Body
        if let Some(ref edit_state) = editing_automation {
            paint_automation_editor(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                &mut scrolls,
                edit_state,
                editing_automation_field,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
            );
        } else if editing_autohide_rules {
            paint_autohide_rules_editor(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                &mut scrolls,
                &snapshot,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
            );
        } else if editing_keybinds {
            paint_keybinds_editor(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                &mut scrolls,
                &snapshot,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
            );
        } else {
            // Tab Bar Header (y: 12..42)
            // Three balanced compact tabs: (420 - 32 - 16) / 3 = 124px each
            let tabs = [
                (FlyoutTab::Services, "Services", scale.rect(16, 12, 140, 42)),
                (FlyoutTab::Options, "Options", scale.rect(148, 12, 272, 42)),
                (FlyoutTab::Settings, "Settings", scale.rect(280, 12, 404, 42)),
            ];

            for (tab, label, rc) in tabs {
                let is_active = tab == active_tab;
                let target_idx = new_targets.len();
                let is_hovered = hovered_idx == Some(target_idx);

                let tab_bg = if is_active {
                    rgb(45, 110, 220) // Accent blue
                } else if is_hovered {
                    rgb(48, 48, 56)
                } else {
                    rgb(35, 35, 40)
                };

                let tab_border = if is_active {
                    rgb(70, 140, 240)
                } else {
                    rgb(50, 50, 58)
                };

                draw_rounded_box(mem_dc, &rc, tab_bg, tab_border, scale.scale(7));

                // Keep font weight normal so text width stays constant on select
                SelectObject(mem_dc, font_normal);
                SetTextColor(mem_dc, if is_active {
                    rgb(255, 255, 255)
                } else {
                    rgb(180, 180, 190)
                });

                let mut text_w = to_wide(label);
                let mut draw_rc = rc;
                DrawTextW(
                    mem_dc,
                    &mut text_w,
                    &mut draw_rc,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );

                new_targets.push(ClickTarget {
                    rect: rc,
                    action: FlyoutAction::SelectTab(tab),
                });
            }

            // Horizontal Separator
            let sep_brush = CreateSolidBrush(rgb(48, 48, 54));
            let sep_rc = scale.rect(16, 48, 404, 49);
            FillRect(mem_dc, &sep_rc, sep_brush);
            let _ = DeleteObject(sep_brush);

            // Tab Body
            match active_tab {
                FlyoutTab::Services => {
                    paint_services_tab(
                        mem_dc,
                        font_title,
                        font_semibold,
                        font_normal,
                        font_small,
                        &scale,
                        &mut scrolls,
                        &snapshot,
                        hovered_idx,
                        &mut new_targets,
                        &mut new_tooltips,
                    );
                }
                FlyoutTab::Options => {
                    paint_options_tab(
                        mem_dc,
                        font_title,
                        font_semibold,
                        font_normal,
                        font_small,
                        &scale,
                        options_subview,
                        &mut scrolls,
                        selected_automation.as_deref(),
                        &snapshot,
                        hovered_idx,
                        &mut new_targets,
                        &mut new_tooltips,
                    );
                }
                FlyoutTab::Settings => {
                    paint_settings_tab(
                        mem_dc,
                        font_title,
                        font_semibold,
                        font_normal,
                        font_small,
                        &scale,
                        &snapshot,
                        keep_gui_open,
                        hovered_idx,
                        &mut new_targets,
                        &mut new_tooltips,
                    );
                }
            }
        }

        // 3. Delete Confirmation Modal (if active, renders over tab or editor)
        if let Some(ref del_name) = confirming_delete {
            draw_delete_confirmation_modal(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                del_name,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
                win_w,
                win_h,
            );
        }

        // 3.5. Interactive Rebind Modal (drawn on top of all content with dimmed backdrop)
        if let Some(ref rebind) = active_rebind {
            draw_rebind_modal(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                rebind,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
                win_w,
                win_h,
            );
        }

        // 3.6. Exit Confirmation Modal
        if confirming_exit {
            draw_exit_confirmation_modal(
                mem_dc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                &scale,
                hovered_idx,
                &mut new_targets,
                &mut new_tooltips,
                win_w,
                win_h,
            );
        }

        // 4. Floating Hover Tooltip (drawn on top of all content)
        if snapshot.show_tooltips {
            if let Some(tooltip_text) = &active_tooltip {
                draw_floating_tooltip(mem_dc, cursor_pos, tooltip_text, font_normal, &scale, win_w, win_h);
            }
        }

        // Transfer to screen
        let _ = BitBlt(hdc, 0, 0, win_w, win_h, mem_dc, 0, 0, SRCCOPY);

        // Cleanup
        SelectObject(mem_dc, old_font);
        let _ = DeleteObject(font_small);
        let _ = DeleteObject(font_normal);
        let _ = DeleteObject(font_semibold);
        let _ = DeleteObject(font_title);

        SelectObject(mem_dc, old_bmp);
        let _ = DeleteObject(mem_bmp);
        let _ = DeleteDC(mem_dc);
    }

    FLYOUT_STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.scrolls = scrolls;
            state.click_targets = new_targets;
            state.tooltips = new_tooltips;
            state.show_tooltips = snapshot.show_tooltips;
        }
    });
}

unsafe fn paint_services_tab(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // Header (Title Capitalization) - Titled Wintil Services
    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide("Wintil Services");
    let mut header_rc = scale.rect(16, 54, 260, 72);
    DrawTextW(hdc, &mut title_w, &mut header_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(150, 150, 160));
    let mut sub_w = to_wide("Core Wintil background services and supervisors");
    let mut sub_rc = scale.rect(16, 72, 404, 88);
    DrawTextW(hdc, &mut sub_w, &mut sub_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Card 1: Native Services (dynamic bounds sized to fit all registered services)
    let card1_top = 92;
    let row_height = 38;
    let service_count = snap.services.len().max(1) as i32;
    let core_card_h = 8 + (service_count * row_height) + 8;
    let core_card_bottom = card1_top + core_card_h;
    let core_card_rc = scale.rect(16, card1_top, 404, core_card_bottom);
    draw_card_frame(hdc, core_card_rc, scale);

    let mut row_base_y = card1_top + 8;
    for service in &snap.services {
        let (dot_color, status_text) = if service.status.contains("Tripped") {
            (rgb(239, 68, 68), format!("✕ {}", service.status))
        } else if service.status == "Deactivated" || service.status.contains("Disabled") {
            (rgb(140, 140, 150), format!("○ {}", service.status))
        } else if service.is_active {
            (rgb(74, 222, 128), format!("● {}", service.status))
        } else {
            (rgb(251, 191, 36), format!("○ {}", service.status))
        };

        // Service Name (Line 1, Left)
        SelectObject(hdc, font_semibold);
        SetTextColor(hdc, rgb(235, 235, 240));
        let mut name_w = to_wide(&service.name);
        let mut name_rc = scale.rect(28, row_base_y + 1, 225, row_base_y + 19);
        DrawTextW(hdc, &mut name_w, &mut name_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

        // Status Pill (Line 1, Right - inline and aligned across ALL rows at 392)
        SelectObject(hdc, font_small);
        SetTextColor(hdc, dot_color);
        let mut st_w = to_wide(&status_text);
        let mut st_rc = scale.rect(225, row_base_y + 1, 392, row_base_y + 19);
        DrawTextW(hdc, &mut st_w, &mut st_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

        // Details line (Line 2, Full width - clean status log)
        let det_rc = scale.rect(28, row_base_y + 19, 392, row_base_y + 36);
        if service.id == "sentry" && !snap.sentry_tripped && (snap.sentry_respawn || snap.sentry_logging) {
            let sentry_kb = (snap.sentry_log_bytes + 1023) / 1024;
            let respawn_word = if snap.sentry_respawn { "on" } else { "off" };
            let respawn_color = if snap.sentry_respawn { rgb(74, 222, 128) } else { rgb(239, 68, 68) };
            let logs_str = if snap.sentry_log_bytes > 0 {
                format!(" • Logs: {} KB ({} errors)", sentry_kb, snap.sentry_log_errors)
            } else {
                " • Logs: Clear".to_string()
            };
            let mut segs: Vec<(&str, COLORREF, windows::Win32::Graphics::Gdi::HFONT)> = vec![
                ("Respawn: ", rgb(160, 160, 170), font_normal),
                (respawn_word, respawn_color, font_semibold),
                (&logs_str, rgb(160, 160, 170), font_normal),
            ];
            if snap.sentry_error_delta == 1 {
                segs.push(("  !", rgb(251, 191, 36), font_semibold));
            } else if snap.sentry_error_delta == 2 {
                segs.push(("  !", rgb(239, 68, 68), font_semibold));
            } else if snap.sentry_error_delta == 3 {
                segs.push(("  !!!", rgb(239, 68, 68), font_semibold));
            } else if snap.sentry_error_delta >= 4 {
                segs.push(("  Critical Errors!", rgb(239, 68, 68), font_semibold));
            }
            draw_colored_segments(hdc, det_rc, &segs);
        } else {
            SetTextColor(hdc, rgb(160, 160, 170));
            SelectObject(hdc, font_normal);
            let mut det_w = to_wide(&service.details);
            let mut d_rc = det_rc;
            DrawTextW(hdc, &mut det_w, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
        }

        row_base_y += row_height;
    }

    // Card 2: Subservices (starts cleanly below Card 1)
    let sub_top = core_card_bottom + 10;
    let sub_bottom = 542;
    let sub_card_rc = scale.rect(16, sub_top, 404, sub_bottom);
    draw_card_frame(hdc, sub_card_rc, scale);

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut s_title_w = to_wide("Subservices");
    let mut st_rc = scale.rect(28, sub_top + 8, 392, sub_top + 28);
    DrawTextW(hdc, &mut s_title_w, &mut st_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    if snap.subservices.is_empty() {
        SelectObject(hdc, font_normal);
        SetTextColor(hdc, rgb(150, 150, 160));
        let mut empty_w = to_wide("No subservices registered in automations.\n\nAdd 'is_subservice = true' to an automation.toml or toggle Subservice Supervisor in editor.");
        let mut emp_rc = scale.rect(28, sub_top + 60, 392, sub_bottom - 20);
        DrawTextW(hdc, &mut empty_w, &mut emp_rc, DT_CENTER | DT_NOPREFIX);
    } else {
        let sub_vp_rc = scale.rect(20, sub_top + 32, 400, sub_bottom - 6);
        let row_step = scale.scale(66);
        let content_h = (snap.subservices.len() as i32) * row_step;
        let is_scrollable = content_h > (sub_vp_rc.bottom - sub_vp_rc.top);
        let text_right = if is_scrollable { 290 } else { 296 };
        let btn_left = if is_scrollable { 298 } else { 304 };
        let btn_right = if is_scrollable { 386 } else { 392 };

        draw_scrollable_section(
            hdc,
            sub_vp_rc,
            content_h,
            ScrollRegionId::ServicesSubservices,
            scrolls,
            scale,
            targets,
            tooltips,
            |scroll_offset, base_idx, _scrolls, inner_targets, _inner_tooltips| {
                for (i, sub) in snap.subservices.iter().enumerate() {
                    let sub_y = sub_vp_rc.top + scale.scale(2) + (i as i32) * row_step - scroll_offset;

                    // Display Name
                    SelectObject(hdc, font_semibold);
                    SetTextColor(hdc, rgb(235, 235, 240));
                    let mut sname_w = to_wide(&sub.display_name);
                    let mut sn_rc = RECT {
                        left: scale.scale(28),
                        top: sub_y,
                        right: scale.scale(text_right),
                        bottom: sub_y + scale.scale(18),
                    };
                    DrawTextW(hdc, &mut sname_w, &mut sn_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Description / Details
                    SelectObject(hdc, font_small);
                    SetTextColor(hdc, rgb(160, 160, 170));
                    let mut sdesc_w = to_wide(&sub.details);
                    let mut sd_rc = RECT {
                        left: scale.scale(28),
                        top: sub_y + scale.scale(18),
                        right: scale.scale(text_right),
                        bottom: sub_y + scale.scale(34),
                    };
                    DrawTextW(hdc, &mut sdesc_w, &mut sd_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Timestamp and Status
                    SetTextColor(hdc, if sub.is_running { rgb(234, 179, 8) } else { rgb(130, 130, 140) });
                    let mut time_w = to_wide(&format!("Last run: {}  |  {}", sub.last_ran_text, sub.status));
                    let mut tr_rc = RECT {
                        left: scale.scale(28),
                        top: sub_y + scale.scale(34),
                        right: scale.scale(text_right),
                        bottom: sub_y + scale.scale(50),
                    };
                    DrawTextW(hdc, &mut time_w, &mut tr_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Action button with compact dimensions (88px wide, 28px tall)
                    let btn_rc = RECT {
                        left: scale.scale(btn_left),
                        top: sub_y + scale.scale(10),
                        right: scale.scale(btn_right),
                        bottom: sub_y + scale.scale(38),
                    };
                    let is_btn_hover = hovered_idx == Some(base_idx + inner_targets.len());
                    let (btn_text, is_active, is_danger) = if sub.is_running {
                        ("Running...", true, false)
                    } else if sub.is_recently_finished {
                        ("Done ✓", true, false)
                    } else {
                        ("Run Fix", false, false)
                    };

                    draw_button(
                        hdc,
                        btn_rc,
                        btn_text,
                        font_normal,
                        scale,
                        is_btn_hover,
                        is_active,
                        is_danger,
                    );

                    if !sub.is_running {
                        inner_targets.push(ClickTarget {
                            rect: btn_rc,
                            action: FlyoutAction::RunAutomation(sub.id.clone()),
                        });
                    }
                }
            },
        );
    }

    // Bottom Status Line (y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(140, 140, 150));
    let active_svc = snap.services.iter().filter(|s| s.is_active).count();
    let sentry_state = if snap.sentry_tripped {
        "tripped"
    } else if snap.sentry_respawn || snap.sentry_logging {
        "active"
    } else {
        "standby"
    };
    let mut stat_w = to_wide(&format!(
        "Services: {}/{} active | Hooks: {} | Sentry: {}",
        active_svc,
        snap.services.len(),
        snap.active_hotkey_count,
        sentry_state
    ));
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

fn get_stroke_color_display(color_name: &str) -> &str {
    match color_name.to_lowercase().as_str() {
        "burgundy" => "Burgundy",
        "crimson" => "Crimson",
        "emerald" => "Emerald",
        "cyan" => "Cyan",
        "blue" => "Royal Blue",
        "amber" => "Amber Gold",
        "violet" => "Violet",
        "white" => "White",
        _ => color_name,
    }
}

unsafe fn paint_options_tab(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    subview: OptionsSubView,
    scrolls: &mut ScrollRegistry,
    selected_automation: Option<&str>,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Submenu Pill Selector (y: 54..82)
    // Order: Keybinds, Window Ops, Automations
    let pills = [
        (OptionsSubView::Keybinds, "Keybinds", scale.rect(16, 54, 140, 82)),
        (OptionsSubView::WindowOps, "Window Ops", scale.rect(148, 54, 272, 82)),
        (OptionsSubView::Automations, "Automations", scale.rect(280, 54, 404, 82)),
    ];

    for (view, label, rc) in pills {
        let is_selected = view == subview;
        let target_idx = targets.len();
        let is_hovered = hovered_idx == Some(target_idx);

        let bg_color = if is_selected {
            rgb(45, 110, 220) // Solid accent blue
        } else if is_hovered {
            rgb(48, 48, 56)
        } else {
            rgb(35, 35, 40)
        };
        let border_color = if is_selected {
            rgb(70, 140, 240)
        } else {
            rgb(50, 50, 58)
        };

        draw_rounded_box(hdc, &rc, bg_color, border_color, scale.scale(7));

        SelectObject(hdc, font_normal);
        SetTextColor(hdc, if is_selected { rgb(255, 255, 255) } else { rgb(180, 180, 190) });
        let mut l_w = to_wide(label);
        let mut d_rc = rc;
        DrawTextW(hdc, &mut l_w, &mut d_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        targets.push(ClickTarget {
            rect: rc,
            action: FlyoutAction::SelectOptionsSubView(view),
        });
    }

    // 2. Render selected subview content
    match subview {
        OptionsSubView::Keybinds => {
            paint_keybinds_subview(
                hdc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                scale,
                scrolls,
                snap,
                hovered_idx,
                targets,
                tooltips,
            );
        }
        OptionsSubView::WindowOps => {
            paint_window_ops_subview(
                hdc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                scale,
                scrolls,
                snap,
                hovered_idx,
                targets,
                tooltips,
            );
        }
        OptionsSubView::Automations => {
            paint_automations_subview(
                hdc,
                font_title,
                font_semibold,
                font_normal,
                font_small,
                scale,
                scrolls,
                snap,
                selected_automation,
                hovered_idx,
                targets,
                tooltips,
            );
        }
    }
}

unsafe fn paint_keybinds_subview(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    _font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // Card: Global Keyboard Shortcuts Legend (y: 90..542)
    let card_rc = scale.rect(16, 90, 404, 542);
    draw_card_frame(hdc, card_rc, scale);

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut hk_title_w = to_wide("Global Keyboard Shortcuts");
    let mut ht_rc = scale.rect(28, 96, 250, 114);
    DrawTextW(hdc, &mut hk_title_w, &mut ht_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (hk_st_col, hk_st_txt) = if snap.active_hotkey_count == 1 {
        (rgb(74, 222, 128), "● 1 Hook Active".to_string())
    } else if snap.active_hotkey_count > 1 {
        (rgb(74, 222, 128), format!("● {} Hooks Active", snap.active_hotkey_count))
    } else {
        (rgb(140, 140, 150), "○ Deactivated".to_string())
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, hk_st_col);
    let mut hk_st_w = to_wide(&hk_st_txt);
    let mut hst_rc = scale.rect(250, 96, 392, 114);
    DrawTextW(hdc, &mut hk_st_w, &mut hst_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(150, 150, 160));
    let mut sub_w = to_wide("Registered system-wide low-level input hooks:");
    let mut sub_rc = scale.rect(28, 114, 392, 130);
    DrawTextW(hdc, &mut sub_w, &mut sub_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let hotkeys: [(&str, &str, &str, &str); 10] = [
        ("toggle_pin", "Pin window always-on-top", "Toggles topmost state and renders process-hugging dynamic border", &snap.hotkeys.toggle_pin),
        ("hide_to_tray", "Hide active window to tray", "Conceals foreground window and adds it to hidden registry", &snap.hotkeys.hide_to_tray),
        ("restore_last", "Restore last hidden window", "Restores most recently hidden window and recovers focus", &snap.hotkeys.restore_last),
        ("restore_all", "Restore all hidden windows", "Restores all currently hidden windows back to desktop", &snap.hotkeys.restore_all),
        ("maximize_toggle", "Toggle maximize and restore", "Maximizes or restores foreground window bounds", &snap.hotkeys.maximize_toggle),
        ("minimize_active", "Minimize active window", "Minimizes active foreground window to taskbar", &snap.hotkeys.minimize_active),
        ("close_active", "Close active window", "Sends WM_CLOSE request to foreground application", &snap.hotkeys.close_active),
        ("kill_active", "Force kill active process", "Immediately terminates active process tree", &snap.hotkeys.kill_active),
        ("kill_all_by_exe", "Force kill all instances", "Terminates all instances sharing the foreground executable", &snap.hotkeys.kill_all_by_exe),
        ("unstick_keys", "Emergency unstick all keys", "Flushes stuck modifier keys (Ctrl, Alt, Shift, Win) and resets state", &snap.hotkeys.unstick_keys),
    ];

    let kb_vp_rc = scale.rect(20, 130, 400, 486);
    let row_step = scale.scale(35);
    let content_h = (hotkeys.len() as i32) * row_step;
    draw_scrollable_section(
        hdc,
        kb_vp_rc,
        content_h,
        ScrollRegionId::KeybindsOverview,
        scrolls,
        scale,
        targets,
        tooltips,
        |scroll_offset, _base_idx, _scrolls, _inner_targets, _inner_tooltips| {
            for (i, (_action_id, desc, _details, bind)) in hotkeys.iter().enumerate() {
                let hk_y = kb_vp_rc.top + scale.scale(2) + (i as i32) * row_step - scroll_offset;
                let display_bind = bind.replace('+', " + ");

                // Spacious Neutral Charcoal Key Combo Pill Badge (28..208 = 180px wide)
                let pill_rc = RECT {
                    left: scale.scale(28),
                    top: hk_y,
                    right: scale.scale(208),
                    bottom: hk_y + scale.scale(24),
                };
                draw_rounded_box(hdc, &pill_rc, rgb(42, 42, 48), rgb(56, 56, 64), scale.scale(5));

                SelectObject(hdc, font_semibold);
                SetTextColor(hdc, rgb(232, 232, 240));
                let mut bind_w = to_wide(&display_bind);
                let mut b_rc = RECT {
                    left: scale.scale(36),
                    top: hk_y,
                    right: scale.scale(200),
                    bottom: hk_y + scale.scale(24),
                };
                DrawTextW(hdc, &mut bind_w, &mut b_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(215, 215, 225));
                let mut desc_w = to_wide(desc);
                let mut d_rc = RECT {
                    left: scale.scale(218),
                    top: hk_y,
                    right: scale.scale(392),
                    bottom: hk_y + scale.scale(24),
                };
                DrawTextW(hdc, &mut desc_w, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            }
        },
    );

    // Button titled Edit at bottom of component (y: 494..532)
    let btn_edit_rc = scale.rect(28, 494, 392, 532);
    let is_edit_hover = hovered_idx == Some(targets.len());
    draw_blue_button(hdc, btn_edit_rc, "Edit Keybinds ⚙", font_normal, scale, is_edit_hover);
    targets.push(ClickTarget {
        rect: btn_edit_rc,
        action: FlyoutAction::StartEditKeybinds,
    });
    tooltips.push(TooltipHit {
        rect: btn_edit_rc,
        text: "Edit Keybinds\nAction: Open interactive configuration panel to remap global shortcuts".to_string(),
    });

    // Bottom Status Line (uniform across panes at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let stat_str = if snap.active_hotkey_count > 0 {
        format!("Global Hotkeys: LowLevel Hook | Zero-Lag Pass-Through | {} Active", snap.active_hotkey_count)
    } else {
        "Global Hotkeys: LowLevel Hook | Deactivated".to_string()
    };
    let mut stat_w = to_wide(&stat_str);
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn paint_keybinds_editor(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Top Header Row (y: 12..40)
    let back_rc = scale.rect(16, 12, 86, 40);
    let is_back_hover = hovered_idx == Some(targets.len());
    draw_blue_button(hdc, back_rc, "< Back", font_normal, scale, is_back_hover);
    targets.push(ClickTarget {
        rect: back_rc,
        action: FlyoutAction::CloseKeybindsEditor,
    });
    tooltips.push(TooltipHit {
        rect: back_rc,
        text: "Navigation: Return to Keybinds overview".to_string(),
    });

    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide("Keybind Configuration");
    let mut title_rc = scale.rect(94, 12, 275, 40);
    DrawTextW(hdc, &mut title_w, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (st_col, st_txt) = if snap.active_hotkey_count == 1 {
        (rgb(74, 222, 128), "● 1 Hook Active".to_string())
    } else if snap.active_hotkey_count > 1 {
        (rgb(74, 222, 128), format!("● {} Hooks Active", snap.active_hotkey_count))
    } else {
        (rgb(140, 140, 150), "○ Deactivated".to_string())
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, st_col);
    let mut st_w = to_wide(&st_txt);
    let mut st_rc = scale.rect(275, 12, 404, 40);
    DrawTextW(hdc, &mut st_w, &mut st_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Separator line
    let sep_brush = CreateSolidBrush(rgb(48, 48, 54));
    let sep_rc = scale.rect(16, 44, 404, 45);
    FillRect(hdc, &sep_rc, sep_brush);
    let _ = DeleteObject(sep_brush);

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(160, 160, 170));
    let mut sub_w = to_wide("Remap global shortcuts or assign new key combinations:");
    let mut sub_rc = scale.rect(16, 48, 404, 66);
    DrawTextW(hdc, &mut sub_w, &mut sub_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Main Card: y: 68..494
    let card_rc = scale.rect(16, 68, 404, 494);
    draw_card_frame(hdc, card_rc, scale);

    let actions: [(&str, &str, &str, &str); 10] = [
        ("toggle_pin", "Pin Window Always-on-Top", "Toggles topmost state & renders border", &snap.hotkeys.toggle_pin),
        ("hide_to_tray", "Hide Active Window to Tray", "Conceals foreground window to hidden registry", &snap.hotkeys.hide_to_tray),
        ("restore_last", "Restore Last Hidden Window", "Restores most recently hidden window", &snap.hotkeys.restore_last),
        ("restore_all", "Restore All Hidden Windows", "Restores all currently hidden windows", &snap.hotkeys.restore_all),
        ("maximize_toggle", "Toggle Maximize / Restore", "Maximizes or restores foreground bounds", &snap.hotkeys.maximize_toggle),
        ("minimize_active", "Minimize Active Window", "Minimizes foreground window to taskbar", &snap.hotkeys.minimize_active),
        ("close_active", "Close Active Window", "Sends WM_CLOSE to foreground application", &snap.hotkeys.close_active),
        ("kill_active", "Force Kill Active Process", "Immediately terminates active process tree", &snap.hotkeys.kill_active),
        ("kill_all_by_exe", "Force Kill All Instances", "Terminates all instances sharing the executable", &snap.hotkeys.kill_all_by_exe),
        ("unstick_keys", "Emergency Unstick All Keys", "Flushes stuck modifier keys (Ctrl/Alt/Shift/Win)", &snap.hotkeys.unstick_keys),
    ];

    let ke_vp_rc = scale.rect(20, 72, 400, 488);
    let row_step = scale.scale(41);
    let content_h = (actions.len() as i32) * row_step;
    draw_scrollable_section(
        hdc,
        ke_vp_rc,
        content_h,
        ScrollRegionId::KeybindsEditor,
        scrolls,
        scale,
        targets,
        tooltips,
        |scroll_offset, base_idx, _scrolls, inner_targets, inner_tooltips| {
            for (i, (action_id, title, desc, bind)) in actions.iter().enumerate() {
                let row_y = ke_vp_rc.top + scale.scale(2) + (i as i32) * row_step - scroll_offset;
                let display_bind = bind.replace('+', " + ");

                // Action Title (aligned at x: 28..214)
                SelectObject(hdc, font_semibold);
                SetTextColor(hdc, rgb(235, 235, 240));
                let mut t_w = to_wide(title);
                let mut t_rc = RECT {
                    left: scale.scale(28),
                    top: row_y + scale.scale(2),
                    right: scale.scale(214),
                    bottom: row_y + scale.scale(19),
                };
                DrawTextW(hdc, &mut t_w, &mut t_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                // Action Subtext (aligned at x: 28..214)
                SelectObject(hdc, font_small);
                SetTextColor(hdc, rgb(140, 140, 150));
                let mut d_w = to_wide(desc);
                let mut d_rc = RECT {
                    left: scale.scale(28),
                    top: row_y + scale.scale(19),
                    right: scale.scale(214),
                    bottom: row_y + scale.scale(35),
                };
                DrawTextW(hdc, &mut d_w, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                // Current Keybind Pill (neutral charcoal, uniform 8px horizontal text inset)
                let pill_rc = RECT {
                    left: scale.scale(220),
                    top: row_y + scale.scale(5),
                    right: scale.scale(316),
                    bottom: row_y + scale.scale(31),
                };
                draw_rounded_box(hdc, &pill_rc, rgb(42, 42, 48), rgb(56, 56, 64), scale.scale(5));

                SelectObject(hdc, font_small);
                SetTextColor(hdc, rgb(232, 232, 240));
                let mut b_w = to_wide(&display_bind);
                let mut b_rc = RECT {
                    left: scale.scale(228),
                    top: row_y + scale.scale(5),
                    right: scale.scale(308),
                    bottom: row_y + scale.scale(31),
                };
                DrawTextW(hdc, &mut b_w, &mut b_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                // [ Rebind ] button (aligned flush at x: 322..392)
                let btn_rebind_rc = RECT {
                    left: scale.scale(322),
                    top: row_y + scale.scale(5),
                    right: scale.scale(392),
                    bottom: row_y + scale.scale(31),
                };
                let is_rb_hover = hovered_idx == Some(base_idx + inner_targets.len());
                draw_button(hdc, btn_rebind_rc, "Rebind", font_small, scale, is_rb_hover, false, false);
                inner_targets.push(ClickTarget {
                    rect: btn_rebind_rc,
                    action: FlyoutAction::OpenRebindModal {
                        action_id: (*action_id).to_string(),
                        action_name: (*title).to_string(),
                        current_bind: (*bind).to_string(),
                    },
                });
                inner_tooltips.push(TooltipHit {
                    rect: btn_rebind_rc,
                    text: format!("Rebind {}\nAction: Click to set a new keybind combination", title),
                });
            }
        },
    );

    // Bottom Action Area: y: 502..538 (aligned at x: 28..392)
    let btn_reset_all_rc = scale.rect(28, 502, 174, 538);
    let is_ra_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_reset_all_rc, "Reset to Defaults", font_normal, scale, is_ra_hover, false, true);
    targets.push(ClickTarget {
        rect: btn_reset_all_rc,
        action: FlyoutAction::ResetDefaultKeybinds,
    });
    tooltips.push(TooltipHit {
        rect: btn_reset_all_rc,
        text: "Reset Defaults\nAction: Restore factory default keybindings for all 10 actions".to_string(),
    });

    let btn_done_rc = scale.rect(296, 502, 392, 538);
    let is_done_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_done_rc, "Done", font_normal, scale, is_done_hover, true, false);
    targets.push(ClickTarget {
        rect: btn_done_rc,
        action: FlyoutAction::CloseKeybindsEditor,
    });
    tooltips.push(TooltipHit {
        rect: btn_done_rc,
        text: "Save & Close\nAction: Return to Keybinds overview".to_string(),
    });

    // Bottom Status Line at y: 550..570
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut stat_w = to_wide("Hotkeys: Zero-Restart Hot Reload | Runtime Re-Registration Active");
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn draw_dimmed_backdrop(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    win_w: i32,
    win_h: i32,
) {
    let mem_dc = CreateCompatibleDC(hdc);
    if mem_dc.is_invalid() {
        return;
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 1,
            biHeight: 1,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
    if let Ok(hbitmap) = dib {
        if !hbitmap.is_invalid() && !bits.is_null() {
            let pixel = bits as *mut u32;
            *pixel = 0x00000000;

            let old_bm = SelectObject(mem_dc, hbitmap);

            let blend = BLENDFUNCTION {
                BlendOp: 0,
                BlendFlags: 0,
                SourceConstantAlpha: 175, // Smooth 68% dark tint
                AlphaFormat: 0,
            };

            let _ = GdiAlphaBlend(
                hdc,
                0,
                0,
                win_w,
                win_h,
                mem_dc,
                0,
                0,
                1,
                1,
                blend,
            );

            SelectObject(mem_dc, old_bm);
            let _ = DeleteObject(hbitmap);
        }
    }
    let _ = DeleteDC(mem_dc);
}

unsafe fn draw_rebind_modal(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    rebind: &RebindModalState,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
    win_w: i32,
    win_h: i32,
) {
    // 1. Darken the background
    draw_dimmed_backdrop(hdc, win_w, win_h);

    // Modal backdrop click-catcher so background elements cannot be clicked through the modal
    let full_window_rc = RECT { left: 0, top: 0, right: win_w, bottom: win_h };
    targets.push(ClickTarget {
        rect: full_window_rc,
        action: FlyoutAction::ResetRebind,
    });

    // 2. Modal Card Frame: neutral dark charcoal with subtle slate border
    let modal_rc = scale.rect(28, 140, 392, 420);
    draw_rounded_box(hdc, &modal_rc, rgb(28, 28, 33), rgb(54, 54, 62), scale.scale(10));

    // Swallow clicks inside the modal card itself that aren't on buttons
    targets.push(ClickTarget {
        rect: modal_rc,
        action: FlyoutAction::Noop,
    });

    // Top Header: "Set New Keybind"
    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut header_w = to_wide("Set New Keybind");
    let mut header_rc = scale.rect(40, 150, 380, 178);
    DrawTextW(hdc, &mut header_w, &mut header_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Text above white box: "use: [box showing input] for: [shortcut]"
    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(185, 185, 195));
    let mut use_w = to_wide("Use:");
    let mut use_rc = scale.rect(40, 180, 380, 202);
    DrawTextW(hdc, &mut use_w, &mut use_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 3. Crisp White Box showing input (tinted red border if conflicting)
    let has_conflict = rebind.conflict_error.is_some();
    let input_box_rc = scale.rect(48, 206, 372, 260);
    let box_border = if has_conflict {
        rgb(220, 50, 50)
    } else {
        rgb(185, 188, 196)
    };
    draw_rounded_box(hdc, &input_box_rc, rgb(248, 248, 250), box_border, scale.scale(8));

    // Inside the white box: show recorded keybind in dark charcoal (or dark red if conflicting)
    SelectObject(hdc, font_title);
    SetTextColor(
        hdc,
        if has_conflict {
            rgb(185, 28, 28)
        } else {
            rgb(24, 24, 28)
        },
    );
    let display_input = if rebind.recorded_bind.is_empty() {
        "Press keys now...".to_string()
    } else {
        rebind.recorded_bind.replace('+', " + ")
    };
    let mut input_w = to_wide(&display_input);
    let mut inp_rc = scale.rect(58, 206, 362, 260);
    DrawTextW(hdc, &mut input_w, &mut inp_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

    // 4. Text below white box: "for: [shortcut]"
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 228));
    let for_text = format!("for: {}", rebind.action_name);
    let mut for_w = to_wide(&for_text);
    let mut for_rc = scale.rect(40, 266, 380, 290);
    DrawTextW(hdc, &mut for_w, &mut for_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

    // Hint or Red Conflict Error text
    let mut hint_rc = scale.rect(40, 292, 380, 316);
    if let Some(ref err_msg) = rebind.conflict_error {
        SelectObject(hdc, font_semibold);
        SetTextColor(hdc, rgb(248, 113, 113));
        let mut err_w = to_wide(err_msg);
        DrawTextW(hdc, &mut err_w, &mut hint_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    } else {
        SelectObject(hdc, font_small);
        SetTextColor(hdc, rgb(140, 140, 150));
        let mut hint_w = to_wide("Press key combination on your keyboard (mouse keys are excluded)");
        DrawTextW(hdc, &mut hint_w, &mut hint_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
    }

    // 5. Two buttons below that: [ Reset ] and [ Confirm ]
    let btn_reset_rc = scale.rect(48, 334, 196, 376);
    let is_reset_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_reset_rc, "Reset", font_semibold, scale, is_reset_hover, false, false);
    targets.push(ClickTarget {
        rect: btn_reset_rc,
        action: FlyoutAction::ResetRebind,
    });
    tooltips.push(TooltipHit {
        rect: btn_reset_rc,
        text: "Reset / Cancel\nDiscards changes and quits rebind modal without altering existing shortcut".to_string(),
    });

    let btn_confirm_rc = scale.rect(224, 334, 372, 376);
    let is_confirm_hover = !has_conflict && hovered_idx == Some(targets.len());
    draw_button(hdc, btn_confirm_rc, "Confirm", font_semibold, scale, is_confirm_hover, !has_conflict, false);
    targets.push(ClickTarget {
        rect: btn_confirm_rc,
        action: if has_conflict {
            FlyoutAction::Noop
        } else {
            FlyoutAction::ConfirmRebind
        },
    });
    tooltips.push(TooltipHit {
        rect: btn_confirm_rc,
        text: if let Some(ref err_msg) = rebind.conflict_error {
            format!("Cannot Confirm\n{}\nAction: Press a different key combination or click Reset", err_msg)
        } else {
            "Confirm Keybind\nCommits new binding to wintil.toml and immediately activates it at runtime".to_string()
        },
    });
}

/// Renders a scrollable viewport using `layout::ScrollState`, clipping GDI drawing,
/// hit targets, and tooltips to `viewport_rc` and drawing a proportional scrollbar thumb.
unsafe fn draw_scrollable_section<F>(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    viewport_rc: RECT,
    content_h: i32,
    id: ScrollRegionId,
    scrolls: &mut ScrollRegistry,
    scale: &DpiScale,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
    draw_content: F,
) where
    F: FnOnce(i32, usize, &mut ScrollRegistry, &mut Vec<ClickTarget>, &mut Vec<TooltipHit>),
{
    let viewport_h = (viewport_rc.bottom - viewport_rc.top).max(0);
    let mut scroll = scrolls
        .states
        .get(&id)
        .map(|(s, _)| *s)
        .unwrap_or_else(|| layout::ScrollState::new(viewport_h, content_h));
    scroll.viewport_height = viewport_h;
    scroll.content_height = content_h.max(0);
    scroll.clamp();
    scrolls.states.insert(id, (scroll, viewport_rc));
    scrolls.draw_order.push(id);

    let saved_dc = SaveDC(hdc);
    let _ = IntersectClipRect(
        hdc,
        viewport_rc.left,
        viewport_rc.top,
        viewport_rc.right,
        viewport_rc.bottom,
    );

    let base_idx = targets.len();
    let mut inner_targets = Vec::new();
    let mut inner_tooltips = Vec::new();
    draw_content(scroll.scroll_offset, base_idx, scrolls, &mut inner_targets, &mut inner_tooltips);

    let vp_lr = layout::LayoutRect::from_win32_rect(&viewport_rc);
    for t in inner_targets {
        let t_lr = layout::LayoutRect::from_win32_rect(&t.rect);
        let clipped_rect = vp_lr
            .intersect(&t_lr)
            .map(|r| r.to_win32_rect())
            .unwrap_or_default();
        targets.push(ClickTarget {
            rect: clipped_rect,
            action: t.action,
        });
    }

    for tt in inner_tooltips {
        let tt_lr = layout::LayoutRect::from_win32_rect(&tt.rect);
        if let Some(clipped) = vp_lr.intersect(&tt_lr) {
            tooltips.push(TooltipHit {
                rect: clipped.to_win32_rect(),
                text: tt.text,
            });
        }
    }

    if scroll.is_scrollable() {
        let sb_w = scale.scale(4).max(2);
        let track_bounds = layout::LayoutRect::new(
            viewport_rc.right - sb_w - scale.scale(2),
            viewport_rc.top + scale.scale(2),
            sb_w,
            (viewport_h - scale.scale(4)).max(0),
        );
        if let Some(thumb) = scroll.thumb_rect(track_bounds, scale.scale(18)) {
            let track_rc = track_bounds.to_win32_rect();
            let track_brush = CreateSolidBrush(rgb(36, 36, 42));
            FillRect(hdc, &track_rc, track_brush);
            let _ = DeleteObject(track_brush);

            let thumb_rc = thumb.to_win32_rect();
            let thumb_brush = CreateSolidBrush(rgb(82, 82, 94));
            FillRect(hdc, &thumb_rc, thumb_brush);
            let _ = DeleteObject(thumb_brush);
        }
    }

    if saved_dc != 0 {
        let _ = RestoreDC(hdc, saved_dc);
    }
}

unsafe fn paint_window_ops_subview(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    _font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    let rules_list_h = if snap.auto_hide_executables.is_empty() {
        24
    } else {
        (snap.auto_hide_executables.len().min(5) as i32) * 20 + 4
    };
    let rules_vp_bot = 732 + rules_list_h;
    let safe_top = rules_vp_bot + 4;
    let safe_bot = safe_top + 18;
    let card4_bot = safe_bot + 8;

    let outer_vp_rc = scale.rect(12, 90, 408, 542);
    let outer_content_h = scale.scale(card4_bot + 6);
    let col_display = get_stroke_color_display(&snap.stroke_color);

    draw_scrollable_section(
        hdc,
        outer_vp_rc,
        outer_content_h,
        ScrollRegionId::WindowOpsPage,
        scrolls,
        scale,
        targets,
        tooltips,
        |outer_off, outer_base_idx, scrolls, targets, tooltips| {
            let y0 = outer_vp_rc.top - outer_off;
            let sy = |dy: i32| y0 + scale.scale(dy);
            let srect = |l: i32, dt: i32, r: i32, db: i32| RECT {
                left: scale.scale(l),
                top: sy(dt),
                right: scale.scale(r),
                bottom: sy(db),
            };

            // ================================================================
            // SECTION 1 (Original Full-Size Pinned Page): rel_y 0..416
            // ================================================================

            // Card 1: Pinned Windows (rel_y: 0..216)
            let pin_rc = srect(16, 0, 398, 216);
            draw_card_frame(hdc, pin_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut pt_w = to_wide(&format!("Pinned Windows ({})", snap.pinned_windows.len()));
            let mut pt_rc = srect(28, 6, 290, 26);
            DrawTextW(hdc, &mut pt_w, &mut pt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            if !snap.pinned_windows.is_empty() {
                let unpin_all_rc = srect(308, 6, 386, 26);
                let is_ua_hover = hovered_idx == Some(outer_base_idx + targets.len());
                draw_button(hdc, unpin_all_rc, "Unpin All", font_small, scale, is_ua_hover, false, true);
                targets.push(ClickTarget {
                    rect: unpin_all_rc,
                    action: FlyoutAction::UnpinAll,
                });
                tooltips.push(TooltipHit {
                    rect: unpin_all_rc,
                    text: "Action: Unpin All\nUnpins all currently pinned windows".to_string(),
                });
            }

            let pin_vp_rc = srect(22, 32, 392, 208);
            if snap.pinned_windows.is_empty() {
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(150, 150, 160));
                let mut msg_w = to_wide("No windows currently pinned.\n\nPress Alt+P on any active window to pin it above others.");
                let mut text_rc = srect(28, 86, 386, 166);
                DrawTextW(hdc, &mut msg_w, &mut text_rc, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
            } else {
                let row_step = scale.scale(42);
                let content_h = (snap.pinned_windows.len() as i32) * row_step;
                let inner_scrollable = content_h > (pin_vp_rc.bottom - pin_vp_rc.top);
                let title_r = if inner_scrollable { 296 } else { 302 };
                let btn_l = if inner_scrollable { 304 } else { 310 };
                let btn_r = if inner_scrollable { 380 } else { 386 };

                draw_scrollable_section(
                    hdc,
                    pin_vp_rc,
                    content_h,
                    ScrollRegionId::WindowOpsPinned,
                    scrolls,
                    scale,
                    targets,
                    tooltips,
                    |scroll_offset, base_idx, _scrolls, inner_targets, _inner_tooltips| {
                        let actual_base = outer_base_idx + base_idx;
                        for (i, item) in snap.pinned_windows.iter().enumerate() {
                            let item_y = pin_vp_rc.top + (i as i32) * row_step - scroll_offset;

                            SelectObject(hdc, font_semibold);
                            SetTextColor(hdc, rgb(235, 235, 240));
                            let mut wtitle = to_wide(&item.title);
                            let mut d_rc = RECT {
                                left: scale.scale(28),
                                top: item_y,
                                right: scale.scale(title_r),
                                bottom: item_y + scale.scale(18),
                            };
                            DrawTextW(hdc, &mut wtitle, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                            let mut wsub = to_wide(&format!("HWND: 0x{:X} | Topmost", item.hwnd));
                            let mut s_rc = RECT {
                                left: scale.scale(28),
                                top: item_y + scale.scale(18),
                                right: scale.scale(title_r),
                                bottom: item_y + scale.scale(34),
                            };
                            SelectObject(hdc, font_small);
                            SetTextColor(hdc, rgb(140, 140, 150));
                            DrawTextW(hdc, &mut wsub, &mut s_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                            let unpin_rc = RECT {
                                left: scale.scale(btn_l),
                                top: item_y + scale.scale(3),
                                right: scale.scale(btn_r),
                                bottom: item_y + scale.scale(29),
                            };
                            let is_un_hover = hovered_idx == Some(actual_base + inner_targets.len());
                            draw_button(hdc, unpin_rc, "Unpin", font_normal, scale, is_un_hover, false, true);
                            inner_targets.push(ClickTarget {
                                rect: unpin_rc,
                                action: FlyoutAction::UnpinHwnd(item.hwnd),
                            });
                        }
                    },
                );
            }

            // Card 2: Pin Border & Indicator Options (rel_y: 224..416)
            let stroke_card_rc = srect(16, 224, 398, 416);
            draw_card_frame(hdc, stroke_card_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut stroke_title_w = to_wide("Pin Border & Indicator Options");
            let mut st_rc = srect(28, 230, 386, 248);
            DrawTextW(hdc, &mut stroke_title_w, &mut st_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let (chk_label, is_act) = if snap.stroke_enabled {
                ("✓ Border Stroke: ON", true)
            } else {
                ("○ Border Stroke: OFF", false)
            };
            let chk_rc = srect(28, 254, 214, 280);
            let is_chk_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, chk_rc, chk_label, font_normal, scale, is_chk_hover, is_act, false);
            targets.push(ClickTarget {
                rect: chk_rc,
                action: FlyoutAction::ToggleStroke,
            });
            tooltips.push(TooltipHit {
                rect: chk_rc,
                text: "Border Stroke: Window border frame overlay\nAction: Click to toggle dynamic border tracking".to_string(),
            });

            let (st_color, st_text) = if !snap.stroke_enabled {
                (rgb(140, 140, 150), "○ Disabled")
            } else if !snap.pinned_windows.is_empty() {
                (rgb(74, 222, 128), "● Tracking Active")
            } else {
                (rgb(251, 191, 36), "○ Standby")
            };
            SelectObject(hdc, font_small);
            SetTextColor(hdc, st_color);
            let mut st_w = to_wide(st_text);
            let mut st_rc2 = srect(220, 256, 386, 278);
            DrawTextW(hdc, &mut st_w, &mut st_rc2, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SelectObject(hdc, font_normal);
            SetTextColor(hdc, rgb(200, 200, 210));
            let mut thick_w = to_wide(&format!("Width: {}px", snap.stroke_thickness));
            let mut th_rc = srect(28, 288, 115, 314);
            DrawTextW(hdc, &mut thick_w, &mut th_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let dec_rc = srect(120, 288, 150, 314);
            draw_button(hdc, dec_rc, "-", font_normal, scale, hovered_idx == Some(outer_base_idx + targets.len()), false, false);
            targets.push(ClickTarget {
                rect: dec_rc,
                action: FlyoutAction::StepStrokeThickness(-1),
            });

            let inc_rc = srect(156, 288, 186, 314);
            draw_button(hdc, inc_rc, "+", font_normal, scale, hovered_idx == Some(outer_base_idx + targets.len()), false, false);
            targets.push(ClickTarget {
                rect: inc_rc,
                action: FlyoutAction::StepStrokeThickness(1),
            });

            let col_btn_text = format!("◼ {}", col_display);
            let col_btn_rc = srect(198, 288, 386, 314);
            let is_col_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, col_btn_rc, &col_btn_text, font_normal, scale, is_col_hover, false, false);
            targets.push(ClickTarget {
                rect: col_btn_rc,
                action: FlyoutAction::CycleStrokeColor,
            });
            tooltips.push(TooltipHit {
                rect: col_btn_rc,
                text: format!("Stroke Color: {}\nAction: Click to cycle border color preset", col_display),
            });

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(140, 140, 150));
            let mut hug_w = to_wide("Hugs window frame dynamically with 0px transparent gap.");
            let mut hug_rc = srect(28, 320, 386, 338);
            DrawTextW(hdc, &mut hug_w, &mut hug_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut hud_title_w = to_wide("Notification Badge");
            let mut ht_rc = srect(28, 348, 386, 366);
            DrawTextW(hdc, &mut hud_title_w, &mut ht_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let (hud_chk_label, is_hud_act) = if snap.hud_enabled {
                ("✓ Pin Badge: ON", true)
            } else {
                ("○ Pin Badge: OFF", false)
            };
            let hud_chk_rc = srect(28, 372, 186, 400);
            draw_button(hdc, hud_chk_rc, hud_chk_label, font_normal, scale, hovered_idx == Some(outer_base_idx + targets.len()), is_hud_act, false);
            targets.push(ClickTarget {
                rect: hud_chk_rc,
                action: FlyoutAction::ToggleHud,
            });
            tooltips.push(TooltipHit {
                rect: hud_chk_rc,
                text: "Notification Badge: HUD indicator shown when pinning windows\nAction: Click to toggle pin badge".to_string(),
            });

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(140, 140, 150));
            let mut hint_w = to_wide("Toast preview badge anchored to flyout");
            let mut hint_rc = srect(194, 372, 386, 400);
            DrawTextW(hdc, &mut hint_w, &mut hint_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

            // ================================================================
            // SECTION 2 (Original Full-Size Hidden Page): rel_y 428..864
            // ================================================================

            // Card 3: Hidden Windows (rel_y: 428..668)
            let hide_rc = srect(16, 428, 398, 668);
            draw_card_frame(hdc, hide_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut ht_w = to_wide(&format!("Hidden Windows ({})", snap.hidden_windows.len()));
            let mut h_rc = srect(28, 434, 386, 454);
            DrawTextW(hdc, &mut ht_w, &mut h_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let hide_vp_rc = srect(22, 460, 392, 624);
            if snap.hidden_windows.is_empty() {
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(150, 150, 160));
                let mut msg_w = to_wide("No windows currently hidden.\n\nPress Alt+Shift+H to hide the active window to tray.");
                let mut text_rc = srect(28, 506, 386, 586);
                DrawTextW(hdc, &mut msg_w, &mut text_rc, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
            } else {
                let row_step = scale.scale(42);
                let content_h = (snap.hidden_windows.len() as i32) * row_step;
                let inner_scrollable = content_h > (hide_vp_rc.bottom - hide_vp_rc.top);
                let title_r = if inner_scrollable { 296 } else { 302 };
                let btn_l = if inner_scrollable { 304 } else { 310 };
                let btn_r = if inner_scrollable { 380 } else { 386 };

                draw_scrollable_section(
                    hdc,
                    hide_vp_rc,
                    content_h,
                    ScrollRegionId::WindowOpsHidden,
                    scrolls,
                    scale,
                    targets,
                    tooltips,
                    |scroll_offset, base_idx, _scrolls, inner_targets, _inner_tooltips| {
                        let actual_base = outer_base_idx + base_idx;
                        for (i, item) in snap.hidden_windows.iter().enumerate() {
                            let item_y = hide_vp_rc.top + (i as i32) * row_step - scroll_offset;

                            SelectObject(hdc, font_semibold);
                            SetTextColor(hdc, rgb(235, 235, 240));
                            let mut wtitle = to_wide(&item.title);
                            let mut d_rc = RECT {
                                left: scale.scale(28),
                                top: item_y,
                                right: scale.scale(title_r),
                                bottom: item_y + scale.scale(18),
                            };
                            DrawTextW(hdc, &mut wtitle, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                            let mut wexe = to_wide(&format!("File: {}", item.exe_name));
                            let mut de_rc = RECT {
                                left: scale.scale(28),
                                top: item_y + scale.scale(18),
                                right: scale.scale(title_r),
                                bottom: item_y + scale.scale(34),
                            };
                            SelectObject(hdc, font_small);
                            SetTextColor(hdc, rgb(140, 140, 150));
                            DrawTextW(hdc, &mut wexe, &mut de_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                            let rest_rc = RECT {
                                left: scale.scale(btn_l),
                                top: item_y + scale.scale(4),
                                right: scale.scale(btn_r),
                                bottom: item_y + scale.scale(30),
                            };
                            let is_r_hover = hovered_idx == Some(actual_base + inner_targets.len());
                            draw_button(hdc, rest_rc, "Restore", font_normal, scale, is_r_hover, true, false);
                            inner_targets.push(ClickTarget {
                                rect: rest_rc,
                                action: FlyoutAction::RestoreHwnd(item.hwnd),
                            });
                        }
                    },
                );
            }

            // Hidden Windows Option Buttons placed BELOW the listed hidden windows (rel_y: 632..660)
            let btn_rl_rc = srect(28, 632, 140, 660);
            draw_button(hdc, btn_rl_rc, "Restore Last", font_small, scale, hovered_idx == Some(outer_base_idx + targets.len()), false, false);
            targets.push(ClickTarget {
                rect: btn_rl_rc,
                action: FlyoutAction::RestoreLast,
            });
            tooltips.push(TooltipHit {
                rect: btn_rl_rc,
                text: "Action: Restore Last\nRestores the most recently hidden window".to_string(),
            });

            let btn_ra_rc = srect(148, 632, 260, 660);
            draw_button(hdc, btn_ra_rc, "Restore All", font_small, scale, hovered_idx == Some(outer_base_idx + targets.len()), false, false);
            targets.push(ClickTarget {
                rect: btn_ra_rc,
                action: FlyoutAction::RestoreAll,
            });
            tooltips.push(TooltipHit {
                rect: btn_ra_rc,
                text: "Action: Restore All\nRestores all hidden windows back to desktop".to_string(),
            });

            let btn_cl_rc = srect(268, 632, 386, 660);
            draw_button(hdc, btn_cl_rc, "Clear List", font_small, scale, hovered_idx == Some(outer_base_idx + targets.len()), false, true);
            targets.push(ClickTarget {
                rect: btn_cl_rc,
                action: FlyoutAction::ClearHidden,
            });
            tooltips.push(TooltipHit {
                rect: btn_cl_rc,
                text: "Action: Clear List\nRemoves all entries from the hidden windows registry".to_string(),
            });

            // Card 4: Auto-Hide Service Rules (dynamic height)
            let service_card_rc = srect(16, 676, 398, card4_bot);
            draw_card_frame(hdc, service_card_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut d_title_w = to_wide("Auto-Hide Service Rules");
            let mut dt_rc = srect(28, 684, 244, 704);
            DrawTextW(hdc, &mut d_title_w, &mut dt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let rules_btn_rc = srect(252, 682, 386, 708);
            let is_rules_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, rules_btn_rc, "Manage Rules ⚙", font_small, scale, is_rules_hover, false, false);
            targets.push(ClickTarget {
                rect: rules_btn_rc,
                action: FlyoutAction::StartEditAutoHideRules,
            });
            tooltips.push(TooltipHit {
                rect: rules_btn_rc,
                text: "Auto-Hide Service Rules\nAction: Open full-panel rules manager to configure background process hiding".to_string(),
            });

            SetTextColor(hdc, rgb(170, 170, 180));
            SelectObject(hdc, font_small);
            let mut desc_w = to_wide("Target executables hidden automatically on detection:");
            let mut desc_rc = srect(28, 712, 386, 728);
            DrawTextW(hdc, &mut desc_w, &mut desc_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let rules_vp_rc = srect(22, 732, 392, rules_vp_bot);
            if snap.auto_hide_executables.is_empty() {
                SetTextColor(hdc, rgb(140, 140, 150));
                let mut none_w = to_wide("  (None configured - background worker unloaded)");
                let mut n_rc = srect(28, 734, 386, 754);
                DrawTextW(hdc, &mut none_w, &mut n_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            } else {
                let row_step = scale.scale(20);
                let content_h = (snap.auto_hide_executables.len() as i32) * row_step;
                draw_scrollable_section(
                    hdc,
                    rules_vp_rc,
                    content_h,
                    ScrollRegionId::WindowOpsRulesPreview,
                    scrolls,
                    scale,
                    targets,
                    tooltips,
                    |scroll_offset, _base_idx, _scrolls, _inner_targets, _inner_tooltips| {
                        for (i, exe) in snap.auto_hide_executables.iter().enumerate() {
                            let exe_y = rules_vp_rc.top + (i as i32) * row_step - scroll_offset;
                            SetTextColor(hdc, rgb(210, 210, 220));
                            SelectObject(hdc, font_small);
                            let mut ex_w = to_wide(&format!("  • {}", exe));
                            let mut ex_rc = RECT {
                                left: scale.scale(28),
                                top: exe_y,
                                right: scale.scale(380),
                                bottom: exe_y + scale.scale(18),
                            };
                            DrawTextW(hdc, &mut ex_w, &mut ex_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                        }
                    },
                );
            }

            SetTextColor(hdc, rgb(140, 140, 150));
            SelectObject(hdc, font_small);
            let mut safe_w = to_wide("Restore all on exit is enabled to prevent orphaned windows.");
            let mut s_rc = srect(28, safe_top, 386, safe_bot);
            DrawTextW(hdc, &mut safe_w, &mut s_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        },
    );

    // Bottom Status Line (uniform across panes at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut stat_w = to_wide(&format!(
        "Pinned: {} | Hidden: {} | Stroke: {}px ({}) | Rules: {}",
        snap.pinned_windows.len(),
        snap.hidden_windows.len(),
        snap.stroke_thickness,
        col_display,
        snap.auto_hide_executables.len()
    ));
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn paint_autohide_rules_editor(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Top Header Row (y: 12..40)
    let back_rc = scale.rect(16, 12, 86, 40);
    let is_back_hover = hovered_idx == Some(targets.len());
    draw_blue_button(hdc, back_rc, "< Back", font_normal, scale, is_back_hover);
    targets.push(ClickTarget {
        rect: back_rc,
        action: FlyoutAction::CloseAutoHideRulesEditor,
    });
    tooltips.push(TooltipHit {
        rect: back_rc,
        text: "Navigation: Return to Window Ops".to_string(),
    });

    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide("Auto-Hide Service Rules");
    let mut title_rc = scale.rect(94, 12, 290, 40);
    DrawTextW(hdc, &mut title_w, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (st_col, st_txt) = if snap.auto_hide_executables.is_empty() {
        (rgb(140, 140, 150), "○ Standby")
    } else {
        (rgb(74, 222, 128), "● Active")
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, st_col);
    let mut st_w = to_wide(st_txt);
    let mut st_rc = scale.rect(290, 12, 404, 40);
    DrawTextW(hdc, &mut st_w, &mut st_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Separator line
    let sep_brush = CreateSolidBrush(rgb(48, 48, 54));
    let sep_rc = scale.rect(16, 44, 404, 45);
    FillRect(hdc, &sep_rc, sep_brush);
    let _ = DeleteObject(sep_brush);

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(160, 160, 170));
    let mut sub_w = to_wide("Executables registered here are automatically concealed to the system tray on detection.");
    let mut sub_rc = scale.rect(16, 48, 404, 66);
    DrawTextW(hdc, &mut sub_w, &mut sub_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let outer_vp_rc = scale.rect(12, 70, 408, 496);
    let outer_content_h = scale.scale(532);

    draw_scrollable_section(
        hdc,
        outer_vp_rc,
        outer_content_h,
        ScrollRegionId::AutoHideRulesPage,
        scrolls,
        scale,
        targets,
        tooltips,
        |outer_off, outer_base_idx, scrolls, targets, tooltips| {
            let y0 = outer_vp_rc.top - outer_off;
            let sy = |dy: i32| y0 + scale.scale(dy);
            let srect = |l: i32, dt: i32, r: i32, db: i32| RECT {
                left: scale.scale(l),
                top: sy(dt),
                right: scale.scale(r),
                bottom: sy(db),
            };

            // Card 1: Currently Registered Rules (rel_y: 0..190)
            let card1_rc = srect(16, 0, 398, 190);
            draw_card_frame(hdc, card1_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut c1_w = to_wide(&format!("Active Rules ({})", snap.auto_hide_executables.len()));
            let mut c1_rc = srect(28, 6, 386, 24);
            DrawTextW(hdc, &mut c1_w, &mut c1_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            if snap.auto_hide_executables.is_empty() {
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(150, 150, 160));
                let mut emp_w = to_wide("No auto-hide executables registered.\n\nSelect a detected app below or configure targets in wintil.toml.");
                let mut emp_rc = srect(28, 50, 386, 140);
                DrawTextW(hdc, &mut emp_w, &mut emp_rc, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
            } else {
                let rules_vp_rc = srect(22, 28, 392, 182);
                let row_step = scale.scale(32);
                let content_h = (snap.auto_hide_executables.len() as i32) * row_step;
                let is_scrollable = content_h > (rules_vp_rc.bottom - rules_vp_rc.top);
                let text_r = if is_scrollable { 288 } else { 294 };
                let btn_l = if is_scrollable { 298 } else { 304 };
                let btn_r = if is_scrollable { 380 } else { 386 };

                draw_scrollable_section(
                    hdc,
                    rules_vp_rc,
                    content_h,
                    ScrollRegionId::AutoHideRulesActive,
                    scrolls,
                    scale,
                    targets,
                    tooltips,
                    |scroll_offset, base_idx, _scrolls, inner_targets, inner_tooltips| {
                        let actual_base = outer_base_idx + base_idx;
                        for (i, exe) in snap.auto_hide_executables.iter().enumerate() {
                            let item_y = rules_vp_rc.top + scale.scale(4) + (i as i32) * row_step - scroll_offset;
                            SelectObject(hdc, font_normal);
                            SetTextColor(hdc, rgb(235, 235, 240));
                            let mut ex_w = to_wide(&format!("•  {}", exe));
                            let mut ex_rc = RECT {
                                left: scale.scale(28),
                                top: item_y,
                                right: scale.scale(text_r),
                                bottom: item_y + scale.scale(24),
                            };
                            DrawTextW(hdc, &mut ex_w, &mut ex_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                            let rm_rc = RECT {
                                left: scale.scale(btn_l),
                                top: item_y,
                                right: scale.scale(btn_r),
                                bottom: item_y + scale.scale(24),
                            };
                            let is_rm_hover = hovered_idx == Some(actual_base + inner_targets.len());
                            draw_button(hdc, rm_rc, "Remove ✕", font_small, scale, is_rm_hover, false, true);
                            inner_targets.push(ClickTarget {
                                rect: rm_rc,
                                action: FlyoutAction::RemoveAutoHideRule(exe.clone()),
                            });
                            inner_tooltips.push(TooltipHit {
                                rect: rm_rc,
                                text: format!("Remove Rule\nAction: Click to deregister {} from automatic hiding", exe),
                            });
                        }
                    },
                );
            }

            // Card 2: Detected Running Desktop Applications (rel_y: 198..430)
            let card2_rc = srect(16, 198, 398, 430);
            draw_card_frame(hdc, card2_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut c2_w = to_wide(&format!("Detected Running Apps ({})", snap.running_apps.len()));
            let mut c2_rc = srect(28, 204, 386, 222);
            DrawTextW(hdc, &mut c2_w, &mut c2_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(150, 150, 160));
            let mut p_desc_w = to_wide("Select a detected application to add it to auto-hide rules:");
            let mut p_desc_rc = srect(28, 224, 386, 242);
            DrawTextW(hdc, &mut p_desc_w, &mut p_desc_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            if snap.running_apps.is_empty() {
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(150, 150, 160));
                let mut emp_app_w = to_wide("No active desktop applications detected.");
                let mut emp_app_rc = srect(28, 290, 386, 340);
                DrawTextW(hdc, &mut emp_app_w, &mut emp_app_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            } else {
                let apps_vp_rc = srect(22, 244, 392, 422);
                let row_step = scale.scale(30);
                let content_h = (snap.running_apps.len() as i32) * row_step;
                let is_scrollable = content_h > (apps_vp_rc.bottom - apps_vp_rc.top);
                let text_r = if is_scrollable { 268 } else { 274 };
                let btn_l = if is_scrollable { 276 } else { 282 };
                let btn_r = if is_scrollable { 380 } else { 386 };

                draw_scrollable_section(
                    hdc,
                    apps_vp_rc,
                    content_h,
                    ScrollRegionId::AutoHideRulesRunning,
                    scrolls,
                    scale,
                    targets,
                    tooltips,
                    |scroll_offset, base_idx, _scrolls, inner_targets, inner_tooltips| {
                        let actual_base = outer_base_idx + base_idx;
                        for (i, app) in snap.running_apps.iter().enumerate() {
                            let row_y = apps_vp_rc.top + scale.scale(2) + (i as i32) * row_step - scroll_offset;
                            SelectObject(hdc, font_normal);
                            SetTextColor(hdc, rgb(235, 235, 240));
                            let mut name_w = to_wide(&app.display_name);
                            let mut name_rc = RECT {
                                left: scale.scale(28),
                                top: row_y,
                                right: scale.scale(text_r),
                                bottom: row_y + scale.scale(26),
                            };
                            DrawTextW(hdc, &mut name_w, &mut name_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                            let btn_rc = RECT {
                                left: scale.scale(btn_l),
                                top: row_y,
                                right: scale.scale(btn_r),
                                bottom: row_y + scale.scale(26),
                            };
                            if app.is_already_ruled {
                                draw_button(hdc, btn_rc, "✓ Active", font_small, scale, false, true, false);
                            } else {
                                let is_btn_h = hovered_idx == Some(actual_base + inner_targets.len());
                                draw_button(hdc, btn_rc, "+ Auto-Hide", font_small, scale, is_btn_h, false, false);
                                inner_targets.push(ClickTarget {
                                    rect: btn_rc,
                                    action: FlyoutAction::AddAutoHideRule(app.target_rule.clone()),
                                });
                                inner_tooltips.push(TooltipHit {
                                    rect: btn_rc,
                                    text: format!(
                                        "Add Auto-Hide Rule\nTarget: {}\nAction: Register for background auto-hiding",
                                        app.target_rule
                                    ),
                                });
                            }
                        }
                    },
                );
            }

            // Card 3: Manual Configuration (rel_y: 438..524)
            let card3_rc = srect(16, 438, 398, 524);
            draw_card_frame(hdc, card3_rc, scale);

            let btn_open_rc = srect(28, 448, 386, 478);
            let is_bo_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, btn_open_rc, "Open wintil.toml for Manual Configuration", font_small, scale, is_bo_hover, false, false);
            targets.push(ClickTarget {
                rect: btn_open_rc,
                action: FlyoutAction::OpenConfig,
            });
            tooltips.push(TooltipHit {
                rect: btn_open_rc,
                text: "Configuration\nAction: Open wintil.toml in text editor to add any custom executable or path target".to_string(),
            });

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(140, 140, 150));
            let mut c_note_w = to_wide("Custom executables can also be specified under [hider.auto_hide_executables].");
            let mut c_note_rc = srect(28, 486, 386, 516);
            DrawTextW(hdc, &mut c_note_w, &mut c_note_rc, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);
        },
    );

    // Card 3 / Bottom action area (y: 498..542)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(140, 140, 150));
    let mut exit_note_w = to_wide("All hidden windows are restored safely upon exiting Wintil.");
    let mut exit_note_rc = scale.rect(28, 502, 280, 534);
    DrawTextW(hdc, &mut exit_note_w, &mut exit_note_rc, DT_LEFT | DT_VCENTER | DT_WORDBREAK | DT_NOPREFIX);

    let done_rc = scale.rect(296, 502, 392, 534);
    let is_done_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, done_rc, "Done", font_normal, scale, is_done_hover, true, false);
    targets.push(ClickTarget {
        rect: done_rc,
        action: FlyoutAction::CloseAutoHideRulesEditor,
    });

    // Bottom Status Line (uniform across panes at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut stat_w = to_wide(&format!("Auto-Hide Service: Running | Monitored Targets: {} | Interval: 500ms", snap.auto_hide_executables.len()));
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn browse_script_file(owner: HWND) -> Option<std::path::PathBuf> {
    let mut file_buf = [0u16; 1024];
    let filter: Vec<u16> = "Automation Scripts (*.ps1;*.bat;*.cmd;*.exe)\0*.ps1;*.bat;*.cmd;*.exe\0All Files (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "Select Automation Script Source\0".encode_utf16().collect();

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: windows::core::PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(file_buf.as_mut_ptr()),
        nMaxFile: file_buf.len() as u32,
        lpstrTitle: windows::core::PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };

    if GetOpenFileNameW(&mut ofn).as_bool() {
        let len = file_buf.iter().position(|&c| c == 0).unwrap_or(file_buf.len());
        let path_str = String::from_utf16_lossy(&file_buf[..len]);
        if !path_str.is_empty() {
            return Some(std::path::PathBuf::from(path_str));
        }
    }
    None
}

unsafe fn paint_automation_editor(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    edit: &crate::automations::AutomationEditState,
    editing_field: Option<AutomationTextField>,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Top Header Row (y: 12..40)
    let back_rc = scale.rect(16, 12, 86, 40);
    let is_back_hover = hovered_idx == Some(targets.len());
    draw_blue_button(hdc, back_rc, "< Back", font_normal, scale, is_back_hover);
    targets.push(ClickTarget { rect: back_rc, action: FlyoutAction::EditorCancel });
    tooltips.push(TooltipHit { rect: back_rc, text: "Navigation: Return to Automations list".to_string() });

    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide(if edit.is_new { "New Automation" } else { "Edit Automation" });
    let mut title_rc = scale.rect(94, 12, 404, 40);
    DrawTextW(hdc, &mut title_w, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Separator line
    let sep_brush = CreateSolidBrush(rgb(48, 48, 54));
    let sep_rc = scale.rect(16, 44, 404, 45);
    FillRect(hdc, &sep_rc, sep_brush);
    let _ = DeleteObject(sep_brush);

    let name_conflict = crate::app_check_automation_conflict(edit);
    let editing_name = editing_field == Some(AutomationTextField::Name);
    let editing_desc = editing_field == Some(AutomationTextField::Description);
    let editing_sub_name = editing_field == Some(AutomationTextField::SubserviceName);

    let meta_h = if name_conflict.is_some() { 126 } else { 112 };
    let sub_top = meta_h + 8;
    let sub_h = 78;
    let trig_top = if edit.is_subservice { sub_top + sub_h + 8 } else { meta_h + 8 };
    let trig_h = if edit.only_manual { 62 } else { 206 };
    let trig_bot = trig_top + trig_h;
    let exec_top = trig_bot + 8;
    let exec_h = if edit.source_script_path.is_some() { 142 } else { 126 };
    let exec_bot = exec_top + exec_h;

    let outer_vp_rc = scale.rect(12, 50, 408, 496);
    let outer_content_h = scale.scale(exec_bot + 6);

    draw_scrollable_section(
        hdc,
        outer_vp_rc,
        outer_content_h,
        ScrollRegionId::AutomationEditorPage,
        scrolls,
        scale,
        targets,
        tooltips,
        |outer_off, outer_base_idx, _scrolls, targets, tooltips| {
            let y0 = outer_vp_rc.top - outer_off;
            let sy = |dy: i32| y0 + scale.scale(dy);
            let srect = |l: i32, dt: i32, r: i32, db: i32| RECT {
                left: scale.scale(l),
                top: sy(dt),
                right: scale.scale(r),
                bottom: sy(db),
            };

            // 2. Metadata & Supervisor Card (rel_y: 0..meta_h)
            let meta_rc = srect(16, 0, 398, meta_h);
            draw_card_frame(hdc, meta_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut m_title_w = to_wide("Metadata & Supervisor");
            let mut mt_rc = srect(28, 6, 386, 22);
            DrawTextW(hdc, &mut m_title_w, &mut mt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SelectObject(hdc, font_normal);
            SetTextColor(hdc, rgb(96, 165, 250));
            let mut n_lbl = to_wide("Name:");
            let mut nl_rc = srect(28, 25, 80, 49);
            DrawTextW(hdc, &mut n_lbl, &mut nl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let nv_box_rc = srect(82, 24, 386, 50);
            let is_nv_hover = hovered_idx == Some(outer_base_idx + targets.len());
            let (nv_bg, nv_border, nv_text_col) = if name_conflict.is_some() {
                (rgb(42, 24, 28), rgb(220, 50, 50), rgb(248, 113, 113))
            } else if editing_name {
                (rgb(22, 24, 30), rgb(59, 130, 246), rgb(240, 240, 245))
            } else if is_nv_hover {
                (rgb(44, 44, 52), rgb(74, 74, 86), rgb(240, 240, 245))
            } else {
                (rgb(38, 38, 44), rgb(54, 54, 62), rgb(240, 240, 245))
            };
            draw_rounded_box(hdc, &nv_box_rc, nv_bg, nv_border, scale.scale(5));
            targets.push(ClickTarget {
                rect: nv_box_rc,
                action: FlyoutAction::EditorFocusName,
            });
            tooltips.push(TooltipHit {
                rect: nv_box_rc,
                text: "Automation Name\nAction: Click to edit name inline (press Enter or click away to finish)".to_string(),
            });

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, nv_text_col);
            let name_display = if editing_name {
                format!("{}|", edit.name)
            } else {
                edit.name.clone()
            };
            let mut n_val = to_wide(&name_display);
            let mut nv_rc = srect(88, 24, 380, 50);
            DrawTextW(hdc, &mut n_val, &mut nv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

            // Description Field below Name (rel_y: 54..78)
            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(150, 150, 160));
            let mut d_lbl = to_wide("Desc:");
            let mut dl_rc = srect(28, 54, 80, 78);
            DrawTextW(hdc, &mut d_lbl, &mut dl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let dv_box_rc = srect(82, 54, 386, 78);
            let is_dv_hover = hovered_idx == Some(outer_base_idx + targets.len());
            let (dv_bg, dv_border) = if editing_desc {
                (rgb(22, 24, 30), rgb(59, 130, 246))
            } else if is_dv_hover {
                (rgb(44, 44, 52), rgb(74, 74, 86))
            } else {
                (rgb(38, 38, 44), rgb(54, 54, 62))
            };
            draw_rounded_box(hdc, &dv_box_rc, dv_bg, dv_border, scale.scale(5));
            targets.push(ClickTarget {
                rect: dv_box_rc,
                action: FlyoutAction::EditorFocusDescription,
            });
            tooltips.push(TooltipHit {
                rect: dv_box_rc,
                text: "Automation Description\nAction: Click to edit description inline".to_string(),
            });

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(210, 210, 220));
            let desc_display = if editing_desc {
                format!("{}|", edit.description)
            } else {
                edit.description.clone()
            };
            let mut d_val = to_wide(&desc_display);
            let mut dv_rc = srect(88, 54, 380, 78);
            DrawTextW(hdc, &mut d_val, &mut dv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

            let mut sup_y = 82;
            if let Some(ref err_msg) = name_conflict {
                SelectObject(hdc, font_small);
                SetTextColor(hdc, rgb(248, 113, 113));
                let mut err_w = to_wide(err_msg);
                let mut err_rc = srect(84, 80, 386, 95);
                DrawTextW(hdc, &mut err_w, &mut err_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                sup_y = 97;
            }

            SelectObject(hdc, font_normal);
            SetTextColor(hdc, rgb(200, 200, 210));
            let mut sub_lbl = to_wide("Subservice Supervisor:");
            let mut subl_rc = srect(28, sup_y, 206, sup_y + 24);
            DrawTextW(hdc, &mut sub_lbl, &mut subl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let sub_btn_rc = srect(216, sup_y, 386, sup_y + 24);
            let is_sub_hover = hovered_idx == Some(outer_base_idx + targets.len());
            let (sub_text, is_sub_act) = if edit.is_subservice { ("Supervisor: ON", true) } else { ("Supervisor: OFF", false) };
            draw_button(hdc, sub_btn_rc, sub_text, font_small, scale, is_sub_hover, is_sub_act, false);
            targets.push(ClickTarget { rect: sub_btn_rc, action: FlyoutAction::EditorToggleSubservice });
            tooltips.push(TooltipHit { rect: sub_btn_rc, text: "Subservice: Supervisor\nRole: Exposes one-click Run Fix trigger and watchdog monitoring in Tab 1 Services".to_string() });

            // 2.5 Conditional Subservice Metadata Card (sub_top..sub_top+sub_h when is_subservice == true)
            if edit.is_subservice {
                let sm_rc = srect(16, sub_top, 398, sub_top + sub_h);
                draw_card_frame(hdc, sm_rc, scale);

                SelectObject(hdc, font_semibold);
                SetTextColor(hdc, rgb(240, 240, 245));
                let mut sm_title = to_wide("Subservice Metadata");
                let mut smt_rc = srect(28, sub_top + 6, 386, sub_top + 22);
                DrawTextW(hdc, &mut sm_title, &mut smt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(96, 165, 250));
                let mut sn_lbl = to_wide("Title:");
                let mut snl_rc = srect(28, sub_top + 26, 80, sub_top + 50);
                DrawTextW(hdc, &mut sn_lbl, &mut snl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let sn_box_rc = srect(82, sub_top + 25, 386, sub_top + 51);
                let is_sn_hover = hovered_idx == Some(outer_base_idx + targets.len());
                let (sn_bg, sn_border) = if editing_sub_name {
                    (rgb(22, 24, 30), rgb(59, 130, 246))
                } else if is_sn_hover {
                    (rgb(44, 44, 52), rgb(74, 74, 86))
                } else {
                    (rgb(38, 38, 44), rgb(54, 54, 62))
                };
                draw_rounded_box(hdc, &sn_box_rc, sn_bg, sn_border, scale.scale(5));
                targets.push(ClickTarget {
                    rect: sn_box_rc,
                    action: FlyoutAction::EditorFocusSubserviceName,
                });
                tooltips.push(TooltipHit {
                    rect: sn_box_rc,
                    text: "Subservice Display Title\nAction: Click to edit the display name shown in Wintil Services -> Subservices".to_string(),
                });

                let sn_empty = edit.subservice_name.is_empty() && !editing_sub_name;
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, if sn_empty { rgb(120, 120, 132) } else { rgb(240, 240, 245) });
                let sn_display = if editing_sub_name {
                    format!("{}|", edit.subservice_name)
                } else if edit.subservice_name.is_empty() {
                    format!("{} (default)", edit.name)
                } else {
                    edit.subservice_name.clone()
                };
                let mut sn_val = to_wide(&sn_display);
                let mut snv_rc = srect(88, sub_top + 25, 380, sub_top + 51);
                DrawTextW(hdc, &mut sn_val, &mut snv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                SelectObject(hdc, font_small);
                SetTextColor(hdc, rgb(140, 140, 150));
                let mut sm_hint = to_wide("Exposes [ Run Fix ] action in Wintil Services -> Subservices");
                let mut smh_rc = srect(28, sub_top + 54, 386, sub_top + 72);
                DrawTextW(hdc, &mut sm_hint, &mut smh_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            }

            // 3. Event Triggers Card (collapses when only_manual == true)
            let trig_rc = srect(16, trig_top, 398, trig_bot);
            draw_card_frame(hdc, trig_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut tr_title = to_wide("Event Triggers (Task Scheduler)");
            let mut trt_rc = srect(28, trig_top + 6, 386, trig_top + 22);
            DrawTextW(hdc, &mut tr_title, &mut trt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            if !edit.only_manual {
                SelectObject(hdc, font_normal);
                SetTextColor(hdc, rgb(210, 210, 220));
                let mut en_lbl = to_wide("Enable Automated Execution:");
                let mut enl_rc = srect(28, trig_top + 28, 236, trig_top + 50);
                DrawTextW(hdc, &mut en_lbl, &mut enl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let en_btn_rc = srect(244, trig_top + 27, 386, trig_top + 51);
                let is_en_hover = hovered_idx == Some(outer_base_idx + targets.len());
                let en_text = if edit.enabled { "Enabled" } else { "Disabled" };
                draw_button(hdc, en_btn_rc, en_text, font_small, scale, is_en_hover, edit.enabled, !edit.enabled);
                targets.push(ClickTarget { rect: en_btn_rc, action: FlyoutAction::EditorToggleEnabled });
                tooltips.push(TooltipHit { rect: en_btn_rc, text: format!("Automated Execution: {}\nAction: Enable or disable scheduled/event-driven runs", en_text) });

                let mut s_lbl = to_wide("System Startup:");
                let mut sl_rc = srect(28, trig_top + 56, 236, trig_top + 78);
                DrawTextW(hdc, &mut s_lbl, &mut sl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let s_btn_rc = srect(244, trig_top + 55, 386, trig_top + 79);
                let is_s_hover = hovered_idx == Some(outer_base_idx + targets.len());
                let (s_text, is_s_act) = if edit.on_startup { ("Startup: ON", true) } else { ("Startup: OFF", false) };
                draw_button(hdc, s_btn_rc, s_text, font_small, scale, is_s_hover, is_s_act, false);
                targets.push(ClickTarget { rect: s_btn_rc, action: FlyoutAction::EditorToggleStartup });
                tooltips.push(TooltipHit { rect: s_btn_rc, text: "Trigger: System Startup\nBehavior: Executes task immediately when system boots or Wintil launches".to_string() });

                let mut w_lbl = to_wide("Sleep / Wake Resume:");
                let mut wl_rc = srect(28, trig_top + 84, 236, trig_top + 106);
                DrawTextW(hdc, &mut w_lbl, &mut wl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let w_btn_rc = srect(244, trig_top + 83, 386, trig_top + 107);
                let is_w_hover = hovered_idx == Some(outer_base_idx + targets.len());
                let (w_text, is_w_act) = if edit.on_wake { ("Wake: ON", true) } else { ("Wake: OFF", false) };
                draw_button(hdc, w_btn_rc, w_text, font_small, scale, is_w_hover, is_w_act, false);
                targets.push(ClickTarget { rect: w_btn_rc, action: FlyoutAction::EditorToggleWake });
                tooltips.push(TooltipHit { rect: w_btn_rc, text: "Trigger: System Wake\nBehavior: Executes task immediately when resuming from sleep or hibernate".to_string() });

                let mut i_lbl = to_wide("Inactivity Resume:");
                let mut il_rc = srect(28, trig_top + 112, 236, trig_top + 134);
                DrawTextW(hdc, &mut i_lbl, &mut il_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let i_btn_rc = srect(244, trig_top + 111, 386, trig_top + 135);
                let is_i_hover = hovered_idx == Some(outer_base_idx + targets.len());
                let (i_text, is_i_act) = if edit.on_inactivity_resume { ("Inactivity: ON", true) } else { ("Inactivity: OFF", false) };
                draw_button(hdc, i_btn_rc, i_text, font_small, scale, is_i_hover, is_i_act, false);
                targets.push(ClickTarget { rect: i_btn_rc, action: FlyoutAction::EditorToggleInactivity });
                tooltips.push(TooltipHit { rect: i_btn_rc, text: "Trigger: Inactivity Resume\nBehavior: Executes task when user resumes activity after reaching idle threshold".to_string() });

                let mut th_lbl = to_wide("Idle Threshold:");
                let mut thl_rc = srect(28, trig_top + 142, 140, trig_top + 166);
                DrawTextW(hdc, &mut th_lbl, &mut thl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                SelectObject(hdc, font_semibold);
                SetTextColor(hdc, rgb(96, 165, 250));
                let mut th_val = to_wide(&format!("{}s", edit.inactivity_threshold_secs));
                let mut thv_rc = srect(142, trig_top + 142, 200, trig_top + 166);
                DrawTextW(hdc, &mut th_val, &mut thv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

                let minus_rc = srect(206, trig_top + 141, 292, trig_top + 167);
                let is_m_hover = hovered_idx == Some(outer_base_idx + targets.len());
                draw_button(hdc, minus_rc, "- 15s", font_small, scale, is_m_hover, false, false);
                targets.push(ClickTarget { rect: minus_rc, action: FlyoutAction::EditorStepInactivity(-15) });
                tooltips.push(TooltipHit { rect: minus_rc, text: "Threshold: Decrease inactivity timeout by 15 seconds".to_string() });

                let plus_rc = srect(300, trig_top + 141, 386, trig_top + 167);
                let is_p_hover = hovered_idx == Some(outer_base_idx + targets.len());
                draw_button(hdc, plus_rc, "+ 15s", font_small, scale, is_p_hover, false, false);
                targets.push(ClickTarget { rect: plus_rc, action: FlyoutAction::EditorStepInactivity(15) });
                tooltips.push(TooltipHit { rect: plus_rc, text: "Threshold: Increase inactivity timeout by 15 seconds".to_string() });
            }

            // Bottom button inside Event Triggers: Only Manual Execution (collapses automated parameters)
            let manual_y = if edit.only_manual { trig_top + 28 } else { trig_top + 173 };
            let manual_rc = srect(28, manual_y, 386, manual_y + 26);
            let is_man_hover = hovered_idx == Some(outer_base_idx + targets.len());
            let man_label = if edit.only_manual {
                "✓ Only Manual Execution"
            } else {
                "Only Manual Execution"
            };
            draw_button(hdc, manual_rc, man_label, font_small, scale, is_man_hover, edit.only_manual, false);
            targets.push(ClickTarget { rect: manual_rc, action: FlyoutAction::EditorToggleManualOnly });
            tooltips.push(TooltipHit {
                rect: manual_rc,
                text: "Only Manual Execution\nAction: Collapse and disable automated triggers; run only via manual trigger".to_string(),
            });

            // 4. Execution & Script Source Card (exec_top..exec_bot)
            let exec_rc = srect(16, exec_top, 398, exec_bot);
            draw_card_frame(hdc, exec_rc, scale);

            SelectObject(hdc, font_semibold);
            SetTextColor(hdc, rgb(240, 240, 245));
            let mut ex_title = to_wide("Execution & Script Source");
            let mut ext_rc = srect(28, exec_top + 6, 386, exec_top + 22);
            DrawTextW(hdc, &mut ex_title, &mut ext_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SelectObject(hdc, font_normal);
            SetTextColor(hdc, rgb(96, 165, 250));
            let mut sc_lbl = to_wide("Script:");
            let mut scl_rc = srect(28, exec_top + 28, 76, exec_top + 50);
            DrawTextW(hdc, &mut sc_lbl, &mut scl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            SetTextColor(hdc, rgb(225, 225, 235));
            let mut sc_val = to_wide(&edit.script);
            let mut scv_rc = srect(78, exec_top + 28, 206, exec_top + 50);
            DrawTextW(hdc, &mut sc_val, &mut scv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

            let br_btn_rc = srect(210, exec_top + 27, 294, exec_top + 51);
            let is_br_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, br_btn_rc, "Browse...", font_small, scale, is_br_hover, false, false);
            targets.push(ClickTarget { rect: br_btn_rc, action: FlyoutAction::EditorBrowseScript });
            tooltips.push(TooltipHit { rect: br_btn_rc, text: "Browse Script\nAction: Open File Explorer to clone an existing script into this automation folder".to_string() });

            let sw_btn_rc = srect(300, exec_top + 27, 386, exec_top + 51);
            let is_sw_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, sw_btn_rc, "Switch Type", font_small, scale, is_sw_hover, false, false);
            targets.push(ClickTarget { rect: sw_btn_rc, action: FlyoutAction::EditorCycleScriptType });
            tooltips.push(TooltipHit { rect: sw_btn_rc, text: "Action: Switch Type\nCycles: PowerShell (.ps1) -> Batch (.bat) -> Command (.cmd)".to_string() });

            SelectObject(hdc, font_small);
            SetTextColor(hdc, rgb(150, 150, 160));
            let mut dir_lbl = to_wide("Folder:");
            let mut dirl_rc = srect(28, exec_top + 56, 76, exec_top + 74);
            DrawTextW(hdc, &mut dir_lbl, &mut dirl_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

            let dir_text = if edit.dir_path.as_os_str().is_empty() {
                "~/.config/wintil/automations/".to_string()
            } else {
                edit.dir_path.to_string_lossy().to_string()
            };
            let mut dir_val = to_wide(&dir_text);
            let mut dirv_rc = srect(78, exec_top + 56, 386, exec_top + 74);
            DrawTextW(hdc, &mut dir_val, &mut dirv_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

            let of_btn_rc = srect(28, exec_top + 82, 203, exec_top + 114);
            let is_of_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, of_btn_rc, "Open Folder", font_small, scale, is_of_hover, false, false);
            targets.push(ClickTarget { rect: of_btn_rc, action: FlyoutAction::EditorOpenFolder });
            tooltips.push(TooltipHit { rect: of_btn_rc, text: "Action: Open Folder\nTarget: Opens task directory in Windows Explorer".to_string() });

            let et_btn_rc = srect(211, exec_top + 82, 386, exec_top + 114);
            let is_et_hover = hovered_idx == Some(outer_base_idx + targets.len());
            draw_button(hdc, et_btn_rc, "Edit TOML", font_small, scale, is_et_hover, false, false);
            targets.push(ClickTarget { rect: et_btn_rc, action: FlyoutAction::EditorOpenToml });
            tooltips.push(TooltipHit { rect: et_btn_rc, text: "Action: Edit TOML\nTarget: Opens automation.toml manifest in Notepad for raw text editing".to_string() });

            if let Some(ref src_p) = edit.source_script_path {
                SelectObject(hdc, font_small);
                SetTextColor(hdc, rgb(74, 222, 128));
                let mut src_w = to_wide(&format!("Clones: {}", src_p.display()));
                let mut src_rc = srect(28, exec_top + 118, 386, exec_top + 136);
                DrawTextW(hdc, &mut src_w, &mut src_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
            }
        },
    );

    // 5. Bottom Action Bar (y: 502..538, Cancel is red!)
    if !edit.is_new {
        let cancel_rc = scale.rect(16, 502, 110, 538);
        let is_can_hover = hovered_idx == Some(targets.len());
        draw_button(hdc, cancel_rc, "Cancel", font_normal, scale, is_can_hover, false, true);
        targets.push(ClickTarget { rect: cancel_rc, action: FlyoutAction::EditorCancel });

        let del_rc = scale.rect(118, 502, 220, 538);
        let is_del_hover = hovered_idx == Some(targets.len());
        draw_button(hdc, del_rc, "Delete", font_normal, scale, is_del_hover, false, true);
        targets.push(ClickTarget { rect: del_rc, action: FlyoutAction::EditorDelete });

        let save_rc = scale.rect(228, 502, 404, 538);
        let is_save_hover = hovered_idx == Some(targets.len()) && name_conflict.is_none();
        draw_button(hdc, save_rc, "Save Automation", font_normal, scale, is_save_hover, name_conflict.is_none(), false);
        targets.push(ClickTarget {
            rect: save_rc,
            action: if name_conflict.is_some() { FlyoutAction::Noop } else { FlyoutAction::EditorSave },
        });
    } else {
        let cancel_rc = scale.rect(16, 502, 195, 538);
        let is_can_hover = hovered_idx == Some(targets.len());
        draw_button(hdc, cancel_rc, "Cancel", font_normal, scale, is_can_hover, false, true);
        targets.push(ClickTarget { rect: cancel_rc, action: FlyoutAction::EditorCancel });

        let save_rc = scale.rect(205, 502, 404, 538);
        let is_save_hover = hovered_idx == Some(targets.len()) && name_conflict.is_none();
        draw_button(hdc, save_rc, "Save Automation", font_normal, scale, is_save_hover, name_conflict.is_none(), false);
        targets.push(ClickTarget {
            rect: save_rc,
            action: if name_conflict.is_some() { FlyoutAction::Noop } else { FlyoutAction::EditorSave },
        });
    }

    // Bottom Status Line (uniform across panes at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut stat_w = to_wide("Automation Editor: Windows Task Scheduler & Event Supervisor");
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn paint_automations_subview(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    _font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    scrolls: &mut ScrollRegistry,
    snap: &crate::FlyoutSnapshot,
    selected_automation: Option<&str>,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Automations List Card (starts directly at y: 90 without redundant tab title, y: 90..542 - reaches 542 for pane uniformity)
    let card_rc = scale.rect(16, 90, 404, 542);
    draw_card_frame(hdc, card_rc, scale);

    if snap.automations.is_empty() {
        SelectObject(hdc, font_normal);
        SetTextColor(hdc, rgb(150, 150, 160));
        let mut msg_w = to_wide("No automations registered.\n\nClick [ Create ] below to add a scheduled task, or place manifests in '.config/wintil/automations/'.");
        let mut text_rc = scale.rect(28, 220, 392, 340);
        DrawTextW(hdc, &mut msg_w, &mut text_rc, DT_CENTER | DT_NOPREFIX);
    } else {
        let auto_vp_rc = scale.rect(18, 96, 402, 488);
        let row_step = scale.scale(78);
        let content_h = (snap.automations.len() as i32) * row_step;
        let is_scrollable = content_h > (auto_vp_rc.bottom - auto_vp_rc.top);
        let sel_r = if is_scrollable { 294 } else { 300 };
        let edit_l = if is_scrollable { 302 } else { 308 };
        let edit_r = if is_scrollable { 386 } else { 392 };
        let vis_r = if is_scrollable { 392 } else { 398 };

        draw_scrollable_section(
            hdc,
            auto_vp_rc,
            content_h,
            ScrollRegionId::AutomationsList,
            scrolls,
            scale,
            targets,
            tooltips,
            |scroll_offset, base_idx, _scrolls, inner_targets, inner_tooltips| {
                for (i, auto) in snap.automations.iter().enumerate() {
                    let item_y = auto_vp_rc.top + scale.scale(2) + (i as i32) * row_step - scroll_offset;
                    let row_visual_rc = RECT {
                        left: scale.scale(22),
                        top: item_y,
                        right: scale.scale(vis_r),
                        bottom: item_y + scale.scale(72),
                    };
                    let row_select_rc = RECT {
                        left: scale.scale(22),
                        top: item_y,
                        right: scale.scale(sel_r),
                        bottom: item_y + scale.scale(72),
                    };
                    let edit_rc = RECT {
                        left: scale.scale(edit_l),
                        top: item_y + scale.scale(12),
                        right: scale.scale(edit_r),
                        bottom: item_y + scale.scale(40),
                    };

                    let is_selected = selected_automation == Some(auto.name.as_str());
                    if is_selected {
                        draw_rounded_box(hdc, &row_visual_rc, rgb(36, 42, 54), rgb(59, 130, 246), scale.scale(7));
                    }

                    // Click target on row for selection
                    inner_targets.push(ClickTarget {
                        rect: row_select_rc,
                        action: FlyoutAction::SelectAutomation(auto.name.clone()),
                    });

                    // Edit button
                    let is_edit_hover = hovered_idx == Some(base_idx + inner_targets.len());
                    draw_button(
                        hdc,
                        edit_rc,
                        "Edit",
                        font_normal,
                        scale,
                        is_edit_hover,
                        false,
                        false,
                    );
                    inner_targets.push(ClickTarget {
                        rect: edit_rc,
                        action: FlyoutAction::StartEditAutomation(auto.name.clone()),
                    });
                    inner_tooltips.push(TooltipHit {
                        rect: edit_rc,
                        text: format!("Action: Edit\nRole: Modify triggers, metadata, and script source for '{}'", auto.name),
                    });

                    // Name
                    SelectObject(hdc, font_semibold);
                    SetTextColor(hdc, rgb(235, 235, 240));
                    let mut wname = to_wide(&auto.name);
                    let mut d_rc = RECT {
                        left: scale.scale(28),
                        top: item_y + scale.scale(2),
                        right: scale.scale(sel_r),
                        bottom: item_y + scale.scale(20),
                    };
                    DrawTextW(hdc, &mut wname, &mut d_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Triggers
                    SelectObject(hdc, font_small);
                    SetTextColor(hdc, rgb(96, 165, 250));
                    let mut trig_w = to_wide(&format!("Triggers: {}", auto.triggers));
                    let mut tr_rc = RECT {
                        left: scale.scale(28),
                        top: item_y + scale.scale(20),
                        right: scale.scale(sel_r),
                        bottom: item_y + scale.scale(36),
                    };
                    DrawTextW(hdc, &mut trig_w, &mut tr_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Description
                    SelectObject(hdc, font_small);
                    SetTextColor(hdc, rgb(160, 160, 170));
                    let mut wdesc = to_wide(&auto.description);
                    let mut de_rc = RECT {
                        left: scale.scale(28),
                        top: item_y + scale.scale(36),
                        right: scale.scale(sel_r),
                        bottom: item_y + scale.scale(52),
                    };
                    DrawTextW(hdc, &mut wdesc, &mut de_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

                    // Status and Script file
                    SetTextColor(hdc, if auto.is_running { rgb(234, 179, 8) } else { rgb(130, 130, 140) });
                    let status_note = if auto.is_running {
                        "Running script..."
                    } else if auto.is_recently_finished {
                        "Executed successfully"
                    } else {
                        "Ready"
                    };
                    let mut time_w = to_wide(&format!("Script: {}  |  Last run: {}  |  {}", auto.script_name, auto.last_ran_text, status_note));
                    let mut tm_rc = RECT {
                        left: scale.scale(28),
                        top: item_y + scale.scale(52),
                        right: scale.scale(edit_r),
                        bottom: item_y + scale.scale(68),
                    };
                    DrawTextW(hdc, &mut time_w, &mut tm_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
                }
            },
        );
    }

    // 2. Action Buttons INSIDE the card at the bottom: [ Create ] (green) and [ Delete ] (red) side by side (y: 496..532)
    let create_rc = scale.rect(28, 496, 206, 532);
    let is_create_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, create_rc, "Create", font_normal, scale, is_create_hover, true, false);
    targets.push(ClickTarget { rect: create_rc, action: FlyoutAction::StartCreateAutomation });
    tooltips.push(TooltipHit {
        rect: create_rc,
        text: "Action: Create\nRole: Open editor to create a new scheduled automation task".to_string(),
    });

    let delete_rc = scale.rect(214, 496, 392, 532);
    let is_delete_hover = hovered_idx == Some(targets.len());
    let has_automations = !snap.automations.is_empty();
    draw_button(hdc, delete_rc, "Delete", font_normal, scale, is_delete_hover, false, has_automations);
    targets.push(ClickTarget { rect: delete_rc, action: FlyoutAction::DeleteSelectedAutomation });
    tooltips.push(TooltipHit {
        rect: delete_rc,
        text: "Action: Delete\nRole: Deletes selected automation from registry and disk with confirmation".to_string(),
    });

    // 3. Bottom Status Line (uniform across panes at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut stat_w = to_wide(&format!("Automations: {} registered | Event supervisor: active", snap.automations.len()));
    let mut stat_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut stat_w, &mut stat_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

unsafe fn paint_settings_tab(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    _font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    snap: &crate::FlyoutSnapshot,
    keep_gui_open: bool,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
) {
    // 1. Windows Startup Task & Privileges Card (y: 54..176)
    let start_card_rc = scale.rect(16, 54, 404, 176);
    draw_card_frame(hdc, start_card_rc, scale);

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut st_title_w = to_wide("Windows Startup Task");
    let mut stt_rc = scale.rect(28, 60, 392, 78);
    DrawTextW(hdc, &mut st_title_w, &mut stt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Row 1: Startup at logon
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut r1_lbl = to_wide("Run at Windows logon");
    let mut r1_rc = scale.rect(28, 86, 185, 108);
    DrawTextW(hdc, &mut r1_lbl, &mut r1_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (st_color, st_status, btn_st_text) = if snap.startup_task.is_enabled {
        (rgb(74, 222, 128), "● Active", "Enabled")
    } else {
        (rgb(150, 150, 160), "○ Disabled", "Enable")
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, st_color);
    let mut st_w = to_wide(st_status);
    let mut st_pill_rc = scale.rect(190, 86, 270, 108);
    DrawTextW(hdc, &mut st_w, &mut st_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_st_rc = scale.rect(275, 84, 392, 108);
    let is_st_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_st_rc, btn_st_text, font_small, scale, is_st_hover, snap.startup_task.is_enabled, false);
    targets.push(ClickTarget {
        rect: btn_st_rc,
        action: FlyoutAction::ToggleStartupTask,
    });
    tooltips.push(TooltipHit {
        rect: btn_st_rc,
        text: "Task: Windows Task Scheduler\nTrigger: At logon\nAction: Click to toggle autostart on boot".to_string(),
    });

    // Row 2: Elevated privileges
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut r2_lbl = to_wide("Elevated");
    let mut r2_rc = scale.rect(28, 118, 185, 140);
    DrawTextW(hdc, &mut r2_lbl, &mut r2_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (el_color, el_status, btn_el_text, is_el_active, is_el_danger, el_tooltip) = if snap.startup_task.is_highest_privilege {
        (
            rgb(74, 222, 128), // Green for elevated
            "▲ Elevated",
            "Highest ▲",
            true,
            false,
            "Elevation (Administrator privileges) is required for window management on elevated windows and running elevated automations/tasks.\nAction: Click to toggle administrative elevation".to_string(),
        )
    } else {
        (
            rgb(239, 68, 68), // Red for non-elevated
            "○ Standard",
            "Elevate ▲",
            false,
            true, // Red alert styling
            "Elevation (Administrator privileges) is required for window management on elevated windows and running elevated automations/tasks.\nAction: Click to elevate with administrator privileges".to_string(),
        )
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, el_color);
    let mut el_w = to_wide(el_status);
    let mut el_pill_rc = scale.rect(190, 118, 270, 140);
    DrawTextW(hdc, &mut el_w, &mut el_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_el_rc = scale.rect(275, 116, 392, 140);
    let is_el_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_el_rc, btn_el_text, font_small, scale, is_el_hover, is_el_active, is_el_danger);
    targets.push(ClickTarget {
        rect: btn_el_rc,
        action: FlyoutAction::ToggleStartupElevation,
    });
    tooltips.push(TooltipHit {
        rect: btn_el_rc,
        text: el_tooltip,
    });

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(140, 140, 150));
    let mut el_hint_w = to_wide("ℹ Elevation is required for operation on certain windows, and running certain automations");
    let mut el_hint_rc = scale.rect(28, 146, 392, 178);
    DrawTextW(hdc, &mut el_hint_w, &mut el_hint_rc, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);

    // 2. Crash Watchdog & Telemetry Card (y: 184..336)
    let sentry_card_rc = scale.rect(16, 184, 404, 336);
    draw_card_frame(hdc, sentry_card_rc, scale);

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut sentry_title_w = to_wide("Crash Watchdog & Telemetry");
    let mut set_rc = scale.rect(28, 190, 230, 208);
    DrawTextW(hdc, &mut sentry_title_w, &mut set_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (sentry_top_color, sentry_top_text) = if snap.sentry_tripped {
        (rgb(239, 68, 68), "✕ Tripped")
    } else if snap.sentry_respawn || snap.sentry_logging {
        (rgb(74, 222, 128), "● Active")
    } else {
        (rgb(150, 150, 160), "○ Standby")
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, sentry_top_color);
    let mut stop_w = to_wide(sentry_top_text);
    let mut stop_rc = scale.rect(230, 190, 392, 208);
    DrawTextW(hdc, &mut stop_w, &mut stop_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Row 1: Auto-Restart on Crash
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut respawn_lbl = to_wide("Auto-restart on crash");
    let mut resp_rc = scale.rect(28, 216, 185, 238);
    DrawTextW(hdc, &mut respawn_lbl, &mut resp_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (resp_color, resp_text, btn_resp_text, is_resp_active) = if snap.sentry_respawn {
        (rgb(74, 222, 128), "● Active", "Respawn: ON", true)
    } else {
        (rgb(150, 150, 160), "○ Disabled", "Respawn: OFF", false)
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, resp_color);
    let mut rw_text = to_wide(resp_text);
    let mut rw_pill_rc = scale.rect(190, 216, 270, 238);
    DrawTextW(hdc, &mut rw_text, &mut rw_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_resp_rc = scale.rect(275, 214, 392, 238);
    let is_resp_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_resp_rc, btn_resp_text, font_small, scale, is_resp_hover, is_resp_active, false);
    targets.push(ClickTarget {
        rect: btn_resp_rc,
        action: FlyoutAction::ToggleSentryRespawn,
    });
    tooltips.push(TooltipHit {
        rect: btn_resp_rc,
        text: "Crash Supervisor: Decoupled watchdog sentry\nFallback: 3 crashes / 60s emergency circuit breaker\nAction: Click to toggle automatic restart upon crash".to_string(),
    });

    // Row 2: Diagnostic Telemetry Logger
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut log_lbl = to_wide("Diagnostic logging");
    let mut log_rc = scale.rect(28, 248, 185, 270);
    DrawTextW(hdc, &mut log_lbl, &mut log_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (log_color, log_text, btn_log_text, is_log_active) = if snap.sentry_logging {
        (rgb(74, 222, 128), "● Active", "Logger: ON", true)
    } else {
        (rgb(150, 150, 160), "○ Disabled", "Logger: OFF", false)
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, log_color);
    let mut lw_text = to_wide(log_text);
    let mut lw_pill_rc = scale.rect(190, 248, 270, 270);
    DrawTextW(hdc, &mut lw_text, &mut lw_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_log_rc = scale.rect(275, 246, 392, 270);
    let is_log_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_log_rc, btn_log_text, font_small, scale, is_log_hover, is_log_active, false);
    targets.push(ClickTarget {
        rect: btn_log_rc,
        action: FlyoutAction::ToggleSentryLogging,
    });
    tooltips.push(TooltipHit {
        rect: btn_log_rc,
        text: "Diagnostic Logger: Master sequential log (logs/wintil.log) + JSONL telemetry\nRate Limit: 2s deduplication window\nAction: Click to toggle diagnostic logging".to_string(),
    });

    // Row 3: Log Footprint & Action Buttons [ Open Logs ] [ Clear Logs ]
    let sentry_kb = (snap.sentry_log_bytes + 1023) / 1024;
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(160, 160, 170));
    let mut foot_w = to_wide(&format!("Logs: {} KB ({} errors)", sentry_kb, snap.sentry_log_errors));
    let mut foot_rc = scale.rect(28, 280, 215, 302);
    DrawTextW(hdc, &mut foot_w, &mut foot_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

    let btn_open_logs_rc = scale.rect(220, 278, 302, 302);
    let is_open_logs_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_open_logs_rc, "Open Logs", font_small, scale, is_open_logs_hover, false, false);
    targets.push(ClickTarget {
        rect: btn_open_logs_rc,
        action: FlyoutAction::OpenLogs,
    });
    tooltips.push(TooltipHit {
        rect: btn_open_logs_rc,
        text: "Diagnostic Logs\nPath: ~/.config/wintil/logs/\nAction: Click to open logs directory in Windows Explorer".to_string(),
    });

    let btn_clr_rc = scale.rect(308, 278, 392, 302);
    let is_clr_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_clr_rc, "Clear Logs", font_small, scale, is_clr_hover, false, true);
    targets.push(ClickTarget {
        rect: btn_clr_rc,
        action: FlyoutAction::ClearLogs,
    });
    tooltips.push(TooltipHit {
        rect: btn_clr_rc,
        text: format!("Diagnostic Logs\nTotal size: {} KB\nRecorded errors: {}\nAction: Click to purge log files and reset circuit breaker", sentry_kb, snap.sentry_log_errors),
    });

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(140, 140, 150));
    let mut foot_hint_w = to_wide("Sequential journal + JSONL telemetry stored in ~/.config/wintil/logs/.");
    let mut foot_hint_rc = scale.rect(28, 310, 392, 328);
    DrawTextW(hdc, &mut foot_hint_w, &mut foot_hint_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 3. System & Application Card (y: 344..542 - lighter grey inner component reaching exactly 542)
    let cfg_card_rc = scale.rect(16, 344, 404, 542);
    draw_card_frame(hdc, cfg_card_rc, scale);

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(240, 240, 245));
    let mut cfg_title_w = to_wide("System & Application");
    let mut ct_rc = scale.rect(28, 350, 210, 368);
    DrawTextW(hdc, &mut cfg_title_w, &mut ct_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(74, 222, 128));
    let mut watch_w = to_wide("● Watcher Active");
    let mut w_rc = scale.rect(210, 350, 392, 368);
    DrawTextW(hdc, &mut watch_w, &mut w_rc, DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(150, 150, 160));
    let mut path_w = to_wide(&format!("File: {}", snap.config_path));
    let mut cp_rc = scale.rect(28, 372, 392, 388);
    DrawTextW(hdc, &mut path_w, &mut cp_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

    // Row 1: On-hover tooltips toggle
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut tt_lbl = to_wide("On-hover tooltips");
    let mut tt_rc = scale.rect(28, 394, 185, 416);
    DrawTextW(hdc, &mut tt_lbl, &mut tt_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (tt_color, tt_status, btn_tt_text, is_tt_active) = if snap.show_tooltips {
        (rgb(74, 222, 128), "● Active", "Tooltips: ON", true)
    } else {
        (rgb(150, 150, 160), "○ Disabled", "Tooltips: OFF", false)
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, tt_color);
    let mut tw_text = to_wide(tt_status);
    let mut tw_pill_rc = scale.rect(190, 394, 266, 416);
    DrawTextW(hdc, &mut tw_text, &mut tw_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_tt_rc = scale.rect(270, 393, 392, 417);
    let is_tt_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_tt_rc, btn_tt_text, font_small, scale, is_tt_hover, is_tt_active, false);
    targets.push(ClickTarget {
        rect: btn_tt_rc,
        action: FlyoutAction::ToggleTooltips,
    });
    tooltips.push(TooltipHit {
        rect: btn_tt_rc,
        text: "UI Preferences\nAction: Click to toggle on-hover guidance tooltips across the application".to_string(),
    });

    // Row 2: Keep GUI open toggle (persists flyout until explicitly closed)
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(220, 220, 225));
    let mut kg_lbl = to_wide("Keep GUI open");
    let mut kg_rc = scale.rect(28, 422, 185, 444);
    DrawTextW(hdc, &mut kg_lbl, &mut kg_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let (kg_color, kg_status, btn_kg_text, is_kg_active) = if keep_gui_open {
        (rgb(74, 222, 128), "● Active", "Keep GUI Open: ON", true)
    } else {
        (rgb(150, 150, 160), "○ Disabled", "Keep GUI Open: OFF", false)
    };
    SelectObject(hdc, font_small);
    SetTextColor(hdc, kg_color);
    let mut kg_st_w = to_wide(kg_status);
    let mut kg_pill_rc = scale.rect(190, 422, 266, 444);
    DrawTextW(hdc, &mut kg_st_w, &mut kg_pill_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    let btn_kg_rc = scale.rect(270, 421, 392, 445);
    let is_kg_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, btn_kg_rc, btn_kg_text, font_small, scale, is_kg_hover, is_kg_active, false);
    targets.push(ClickTarget {
        rect: btn_kg_rc,
        action: FlyoutAction::ToggleKeepGuiOpen,
    });
    tooltips.push(TooltipHit {
        rect: btn_kg_rc,
        text: "UI Preferences\nAction: Keep the flyout window open when clicking outside until closed via tray icon or Esc".to_string(),
    });

    // Row 3 Buttons: [ Open Config ]  [ Reload Config ]  [ Unstick Keys ]
    let btn_open_rc = scale.rect(28, 454, 144, 484);
    draw_button(
        hdc,
        btn_open_rc,
        "Open Config",
        font_small,
        scale,
        hovered_idx == Some(targets.len()),
        false,
        false,
    );
    targets.push(ClickTarget {
        rect: btn_open_rc,
        action: FlyoutAction::OpenConfig,
    });
    tooltips.push(TooltipHit {
        rect: btn_open_rc,
        text: "Configuration\nAction: Open wintil.toml in system text editor".to_string(),
    });

    let btn_rel_rc = scale.rect(150, 454, 268, 484);
    draw_button(
        hdc,
        btn_rel_rc,
        "Reload Config",
        font_small,
        scale,
        hovered_idx == Some(targets.len()),
        false,
        false,
    );
    targets.push(ClickTarget {
        rect: btn_rel_rc,
        action: FlyoutAction::ReloadConfig,
    });
    tooltips.push(TooltipHit {
        rect: btn_rel_rc,
        text: "Configuration\nAction: Hot-reload active configuration from disk".to_string(),
    });

    let btn_flush_rc = scale.rect(274, 454, 392, 484);
    draw_button(
        hdc,
        btn_flush_rc,
        "Unstick Keys",
        font_small,
        scale,
        hovered_idx == Some(targets.len()),
        false,
        false,
    );
    targets.push(ClickTarget {
        rect: btn_flush_rc,
        action: FlyoutAction::FlushModifiers,
    });
    tooltips.push(TooltipHit {
        rect: btn_flush_rc,
        text: "Keyboard Buffer Recovery\nFlushes latched modifier keys (Ctrl, Alt, Shift, Win)\nAction: Click to flush keyboard buffer".to_string(),
    });

    // Row 4: Refined Exit Button INSIDE the lighter grey card component (y: 492..532)
    let exit_rc = scale.rect(28, 492, 392, 532);
    let is_exit_hover = hovered_idx == Some(targets.len());
    draw_button(
        hdc,
        exit_rc,
        "Exit Wintil",
        font_normal,
        scale,
        is_exit_hover,
        false,
        true,
    );
    targets.push(ClickTarget {
        rect: exit_rc,
        action: FlyoutAction::RequestExitApp,
    });
    tooltips.push(TooltipHit {
        rect: exit_rc,
        text: "Application Lifecycle\nAction: Prompts confirmation to gracefully restore all windows and terminate Wintil".to_string(),
    });

    // 4. Version label (centered at y: 550..570)
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(130, 130, 140));
    let mut ver_w = to_wide(&format!("Wintil, v{}", env!("CARGO_PKG_VERSION")));
    let mut v_rc = scale.rect(16, 550, 404, 570);
    DrawTextW(hdc, &mut ver_w, &mut v_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

struct MeasuredLine<'a> {
    key: Option<&'a str>,
    val: &'a str,
    key_w: i32,
    val_w: i32,
    val_h: i32,
    line_w: i32,
    total_line_h: i32,
}

unsafe fn draw_floating_tooltip(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    cursor: POINT,
    text: &str,
    font: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    win_w: i32,
    win_h: i32,
) {
    SelectObject(hdc, font);
    let pad_x = scale.scale(10);
    let pad_y = scale.scale(8);
    let line_h = scale.scale(18);
    let gap = scale.scale(6);

    let max_box_w = (win_w - scale.scale(16)).max(scale.scale(180));
    let max_content_w = max_box_w - pad_x * 2;

    let raw_lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if raw_lines.is_empty() {
        return;
    }

    let mut measured: Vec<MeasuredLine> = Vec::new();

    for l in &raw_lines {
        if let Some((k, v)) = l.split_once(": ") {
            let k_trim = k.trim();
            let v_trim = v.trim();

            let mut kw_wide = to_wide(&format!("{}:", k_trim));
            let mut k_rc = RECT::default();
            DrawTextW(hdc, &mut kw_wide, &mut k_rc, DT_CALCRECT | DT_NOPREFIX);
            let kw = k_rc.right - k_rc.left;

            let val_avail_w = (max_content_w - kw - gap).max(scale.scale(80));
            let mut vw_wide = to_wide(v_trim);
            let mut v_rc = RECT {
                left: 0,
                top: 0,
                right: val_avail_w,
                bottom: 0,
            };
            DrawTextW(hdc, &mut vw_wide, &mut v_rc, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            let vw = (v_rc.right - v_rc.left).min(val_avail_w);
            let vh = (v_rc.bottom - v_rc.top).max(line_h);

            measured.push(MeasuredLine {
                key: Some(k_trim),
                val: v_trim,
                key_w: kw,
                val_w: vw,
                val_h: vh,
                line_w: kw + gap + vw,
                total_line_h: vh,
            });
        } else {
            let mut tw_wide = to_wide(l);
            let mut t_rc = RECT {
                left: 0,
                top: 0,
                right: max_content_w,
                bottom: 0,
            };
            DrawTextW(hdc, &mut tw_wide, &mut t_rc, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            let tw = (t_rc.right - t_rc.left).min(max_content_w);
            let th = (t_rc.bottom - t_rc.top).max(line_h);

            measured.push(MeasuredLine {
                key: None,
                val: l,
                key_w: 0,
                val_w: tw,
                val_h: th,
                line_w: tw,
                total_line_h: th,
            });
        }
    }

    let content_w = measured.iter().map(|m| m.line_w).max().unwrap_or(scale.scale(60));
    let box_w = (content_w + pad_x * 2).min(max_box_w);
    let total_content_h: i32 = measured.iter().map(|m| m.total_line_h).sum();
    let box_h = total_content_h + pad_y * 2;

    let min_left = scale.scale(8);
    let max_right = win_w - scale.scale(8);

    // Dynamic horizontal placement:
    // 1. Try placing to the right of the cursor
    // 2. If it overflows right, try placing to the left of the cursor
    // 3. If too large for either, anchor right edge flush at max_right
    let right_pos = cursor.x + scale.scale(12);
    let left_pos = cursor.x - box_w - scale.scale(8);

    let tip_x = if right_pos + box_w <= max_right {
        right_pos
    } else if left_pos >= min_left {
        left_pos
    } else {
        (max_right - box_w).max(min_left)
    };

    let min_top = scale.scale(8);
    let max_bottom = win_h - scale.scale(8);

    // Dynamic vertical placement:
    // 1. Try below cursor
    // 2. If it overflows bottom, try above cursor
    // 3. If too tall, anchor bottom edge flush at max_bottom
    let down_pos = cursor.y + scale.scale(18);
    let up_pos = cursor.y - box_h - scale.scale(8);

    let tip_y = if down_pos + box_h <= max_bottom {
        down_pos
    } else if up_pos >= min_top {
        up_pos
    } else {
        (max_bottom - box_h).max(min_top)
    };

    let box_rc = RECT {
        left: tip_x,
        top: tip_y,
        right: tip_x + box_w,
        bottom: tip_y + box_h,
    };

    draw_rounded_box(
        hdc,
        &box_rc,
        rgb(62, 62, 70),
        rgb(88, 88, 98),
        scale.scale(6),
    );

    let mut curr_y = tip_y + pad_y;
    for item in &measured {
        if let Some(k) = item.key {
            // Key in bright cool silver for crisp contrast
            SelectObject(hdc, font);
            SetTextColor(hdc, rgb(205, 210, 222));
            let mut kw_wide = to_wide(&format!("{}:", k));
            let mut k_rc = RECT {
                left: tip_x + pad_x,
                top: curr_y,
                right: tip_x + pad_x + item.key_w,
                bottom: curr_y + line_h,
            };
            DrawTextW(hdc, &mut kw_wide, &mut k_rc, DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX);

            // Value in crisp white, starting immediately after this line's key
            SetTextColor(hdc, rgb(250, 250, 255));
            let mut vw_wide = to_wide(item.val);
            let val_x = tip_x + pad_x + item.key_w + gap;
            let mut v_rc = RECT {
                left: val_x,
                top: curr_y,
                right: (val_x + item.val_w).min(tip_x + box_w - pad_x),
                bottom: curr_y + item.val_h,
            };
            DrawTextW(hdc, &mut vw_wide, &mut v_rc, DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX);
        } else {
            SelectObject(hdc, font);
            SetTextColor(hdc, rgb(250, 250, 255));
            let mut tw_wide = to_wide(item.val);
            let mut t_rc = RECT {
                left: tip_x + pad_x,
                top: curr_y,
                right: tip_x + box_w - pad_x,
                bottom: curr_y + item.val_h,
            };
            DrawTextW(hdc, &mut tw_wide, &mut t_rc, DT_LEFT | DT_TOP | DT_WORDBREAK | DT_NOPREFIX);
        }
        curr_y += item.total_line_h;
    }
}

unsafe fn draw_delete_confirmation_modal(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    target_name: &str,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
    win_w: i32,
    win_h: i32,
) {
    // 1. Darken the background
    draw_dimmed_backdrop(hdc, win_w, win_h);

    let full_rc = RECT { left: 0, top: 0, right: win_w, bottom: win_h };
    targets.push(ClickTarget {
        rect: full_rc,
        action: FlyoutAction::CancelDelete,
    });

    // 2. Modal Card Container (neutral dark charcoal with subtle slate border)
    let modal_rc = scale.rect(28, 160, 392, 400);
    draw_rounded_box(hdc, &modal_rc, rgb(28, 28, 33), rgb(54, 54, 62), scale.scale(10));
    targets.push(ClickTarget {
        rect: modal_rc,
        action: FlyoutAction::Noop,
    });

    // 3. Danger Badge
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(248, 113, 113));
    let mut badge_w = to_wide("CONFIRM DELETION  |  PERMANENT ACTION");
    let mut badge_rc = scale.rect(44, 174, 376, 192);
    DrawTextW(hdc, &mut badge_w, &mut badge_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 4. Modal Title
    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide("Delete Automation Task?");
    let mut title_rc = scale.rect(44, 194, 376, 220);
    DrawTextW(hdc, &mut title_w, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 5. Prompt Message
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(200, 200, 210));
    let mut msg_w = to_wide("Are you sure you want to permanently delete:");
    let mut msg_rc = scale.rect(44, 224, 376, 244);
    DrawTextW(hdc, &mut msg_w, &mut msg_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 6. Highlighted Task Name Card
    let name_card_rc = scale.rect(44, 248, 376, 286);
    draw_rounded_box(hdc, &name_card_rc, rgb(38, 38, 44), rgb(56, 56, 64), scale.scale(6));

    SelectObject(hdc, font_semibold);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut name_w = to_wide(target_name);
    let mut name_rc = scale.rect(54, 248, 366, 286);
    DrawTextW(hdc, &mut name_w, &mut name_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);

    // 7. Warning Subtext
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(155, 155, 165));
    let mut warn_w = to_wide("The automation manifest, scheduled triggers, and task directory will be removed.");
    let mut warn_rc = scale.rect(44, 292, 376, 328);
    DrawTextW(hdc, &mut warn_w, &mut warn_rc, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);

    // 8. Action Buttons
    let cancel_rc = scale.rect(44, 342, 204, 384);
    let is_can_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, cancel_rc, "Cancel", font_normal, scale, is_can_hover, false, false);
    targets.push(ClickTarget { rect: cancel_rc, action: FlyoutAction::CancelDelete });
    tooltips.push(TooltipHit { rect: cancel_rc, text: "Cancel: Keep automation task unchanged".to_string() });

    let confirm_rc = scale.rect(216, 342, 376, 384);
    let is_conf_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, confirm_rc, "Confirm Delete", font_normal, scale, is_conf_hover, false, true);
    targets.push(ClickTarget { rect: confirm_rc, action: FlyoutAction::ConfirmDelete });
    tooltips.push(TooltipHit { rect: confirm_rc, text: format!("Confirm: Permanently delete '{}'", target_name) });
}

unsafe fn draw_exit_confirmation_modal(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    font_title: windows::Win32::Graphics::Gdi::HFONT,
    _font_semibold: windows::Win32::Graphics::Gdi::HFONT,
    font_normal: windows::Win32::Graphics::Gdi::HFONT,
    font_small: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    hovered_idx: Option<usize>,
    targets: &mut Vec<ClickTarget>,
    tooltips: &mut Vec<TooltipHit>,
    win_w: i32,
    win_h: i32,
) {
    // 1. Darken the background
    draw_dimmed_backdrop(hdc, win_w, win_h);

    let full_rc = RECT { left: 0, top: 0, right: win_w, bottom: win_h };
    targets.push(ClickTarget {
        rect: full_rc,
        action: FlyoutAction::CancelExitApp,
    });

    // 2. Modal Card Container (neutral dark charcoal with subtle slate border)
    let modal_rc = scale.rect(28, 168, 392, 392);
    draw_rounded_box(hdc, &modal_rc, rgb(28, 28, 33), rgb(54, 54, 62), scale.scale(10));
    targets.push(ClickTarget {
        rect: modal_rc,
        action: FlyoutAction::Noop,
    });

    // 3. Header Badge
    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(248, 113, 113));
    let mut badge_w = to_wide("APPLICATION LIFECYCLE  |  CONFIRM EXIT");
    let mut badge_rc = scale.rect(44, 182, 376, 200);
    DrawTextW(hdc, &mut badge_w, &mut badge_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 4. Modal Title
    SelectObject(hdc, font_title);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut title_w = to_wide("Exit Wintil?");
    let mut title_rc = scale.rect(44, 204, 376, 230);
    DrawTextW(hdc, &mut title_w, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 5. Prompt Message
    SelectObject(hdc, font_normal);
    SetTextColor(hdc, rgb(215, 215, 225));
    let mut msg_w = to_wide("Are you sure you want to close Wintil?");
    let mut msg_rc = scale.rect(44, 236, 376, 258);
    DrawTextW(hdc, &mut msg_w, &mut msg_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // 6. Subtext card
    let info_card_rc = scale.rect(44, 266, 376, 316);
    draw_rounded_box(hdc, &info_card_rc, rgb(38, 38, 44), rgb(56, 56, 64), scale.scale(6));

    SelectObject(hdc, font_small);
    SetTextColor(hdc, rgb(175, 178, 188));
    let mut warn_w = to_wide("All hidden and pinned windows will be safely restored to your desktop before Wintil terminates.");
    let mut warn_rc = scale.rect(54, 274, 366, 310);
    DrawTextW(hdc, &mut warn_w, &mut warn_rc, DT_LEFT | DT_WORDBREAK | DT_NOPREFIX);

    // 7. Action Buttons
    let cancel_rc = scale.rect(44, 332, 204, 374);
    let is_can_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, cancel_rc, "Cancel", font_normal, scale, is_can_hover, false, false);
    targets.push(ClickTarget { rect: cancel_rc, action: FlyoutAction::CancelExitApp });
    tooltips.push(TooltipHit { rect: cancel_rc, text: "Cancel: Keep Wintil running in the system tray".to_string() });

    let confirm_rc = scale.rect(216, 332, 376, 374);
    let is_conf_hover = hovered_idx == Some(targets.len());
    draw_button(hdc, confirm_rc, "Exit Wintil", font_normal, scale, is_conf_hover, false, true);
    targets.push(ClickTarget { rect: confirm_rc, action: FlyoutAction::ConfirmExitApp });
    tooltips.push(TooltipHit { rect: confirm_rc, text: "Confirm: Restore all windows and exit Wintil".to_string() });
}

unsafe fn draw_rounded_box(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: &RECT,
    bg_color: COLORREF,
    border_color: COLORREF,
    radius: i32,
) {
    let pen = CreatePen(PS_SOLID, 1, border_color);
    let brush = CreateSolidBrush(bg_color);
    let old_pen = SelectObject(hdc, pen);
    let old_brush = SelectObject(hdc, brush);

    let diam = radius.max(2) * 2;
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, diam, diam);

    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(pen);
    let _ = DeleteObject(brush);
}

pub(crate) unsafe fn draw_card_frame(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: RECT,
    scale: &DpiScale,
) {
    draw_rounded_box(
        hdc,
        &rc,
        rgb(32, 32, 37),
        rgb(50, 50, 58),
        scale.scale(10),
    );
}

pub(crate) unsafe fn draw_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: RECT,
    text: &str,
    font: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    is_hovered: bool,
    is_active: bool,
    is_danger: bool,
) {
    let bg_color = if is_danger {
        if is_hovered {
            rgb(170, 48, 48)
        } else {
            rgb(132, 34, 34)
        }
    } else if is_active {
        if is_hovered {
            rgb(38, 140, 75)
        } else {
            rgb(28, 114, 62)
        }
    } else if is_hovered {
        rgb(54, 54, 62)
    } else {
        rgb(42, 42, 48)
    };

    let border_color = if is_danger {
        rgb(165, 50, 50)
    } else if is_active {
        rgb(44, 148, 80)
    } else if is_hovered {
        rgb(68, 68, 78)
    } else {
        rgb(54, 54, 62)
    };

    draw_styled_button(hdc, rc, text, font, scale, bg_color, border_color);
}

pub(crate) unsafe fn draw_blue_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: RECT,
    text: &str,
    font: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    is_hovered: bool,
) {
    let bg_color = if is_hovered {
        rgb(58, 125, 235)
    } else {
        rgb(45, 110, 220)
    };
    draw_styled_button(hdc, rc, text, font, scale, bg_color, rgb(70, 140, 240));
}

unsafe fn draw_styled_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rc: RECT,
    text: &str,
    font: windows::Win32::Graphics::Gdi::HFONT,
    scale: &DpiScale,
    bg_color: COLORREF,
    border_color: COLORREF,
) {
    draw_rounded_box(
        hdc,
        &rc,
        bg_color,
        border_color,
        scale.scale(8),
    );

    SelectObject(hdc, font);
    SetTextColor(hdc, rgb(245, 245, 250));
    let mut text_w = to_wide(text);
    let pad_x = scale.scale(6);
    let mut draw_rc = RECT {
        left: (rc.left + pad_x).min(rc.right),
        top: rc.top,
        right: (rc.right - pad_x).max(rc.left),
        bottom: rc.bottom,
    };
    DrawTextW(
        hdc,
        &mut text_w,
        &mut draw_rc,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
}

pub(crate) unsafe fn draw_colored_segments(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    bounds: RECT,
    segments: &[(&str, COLORREF, windows::Win32::Graphics::Gdi::HFONT)],
) {
    let mut x = bounds.left;
    for (text, color, font) in segments {
        if x >= bounds.right {
            break;
        }
        SelectObject(hdc, *font);
        let mut wide = to_wide(text);
        let mut meas_rc = RECT {
            left: x,
            top: bounds.top,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        DrawTextW(hdc, &mut wide, &mut meas_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_CALCRECT | DT_NOPREFIX);
        let seg_w = meas_rc.right - meas_rc.left;
        let mut draw_rc = RECT {
            left: x,
            top: bounds.top,
            right: (x + seg_w).min(bounds.right),
            bottom: bounds.bottom,
        };
        SetTextColor(hdc, *color);
        DrawTextW(hdc, &mut wide, &mut draw_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
        x += seg_w;
    }
}

pub(crate) fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_bottom_right_standard() {
        let mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            rcWork: RECT { left: 0, top: 0, right: 1920, bottom: 1040 },
            dwFlags: 1,
        };
        let scale = DpiScale { dpi: 96 };
        let click_pt = Some(POINT { x: 1850, y: 1060 });
        let (x, y, w, h, dock_top) = get_flyout_geometry(&mi, &scale, click_pt);

        assert!(!dock_top);
        assert_eq!(w, 420);
        assert_eq!(h, 580);
        assert_eq!(x, 1920 - 420 - 12);
        assert_eq!(y, 1040 - 580 - 12);
    }

    #[test]
    fn test_geometry_top_taskbar() {
        let mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            rcWork: RECT { left: 0, top: 40, right: 1920, bottom: 1080 },
            dwFlags: 1,
        };
        let scale = DpiScale { dpi: 96 };
        let click_pt = Some(POINT { x: 1850, y: 20 });
        let (x, y, w, h, dock_top) = get_flyout_geometry(&mi, &scale, click_pt);

        assert!(dock_top);
        assert_eq!(w, 420);
        assert_eq!(h, 580);
        assert_eq!(x, 1920 - 420 - 12);
        assert_eq!(y, 40 + 12);
    }

    #[test]
    fn test_geometry_left_taskbar() {
        let mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            rcWork: RECT { left: 70, top: 0, right: 1920, bottom: 1080 },
            dwFlags: 1,
        };
        let scale = DpiScale { dpi: 96 };
        let click_pt = Some(POINT { x: 35, y: 1040 });
        let (x, y, w, h, dock_top) = get_flyout_geometry(&mi, &scale, click_pt);

        assert!(!dock_top);
        assert_eq!(w, 420);
        assert_eq!(h, 580);
        assert_eq!(x, 70 + 12);
        assert_eq!(y, 1080 - 580 - 12);
    }

    #[test]
    fn test_geometry_top_left_tray_click() {
        let mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT { left: 0, top: 0, right: 1920, bottom: 1080 },
            rcWork: RECT { left: 0, top: 0, right: 1920, bottom: 1080 }, // Autohidden or secondary
            dwFlags: 0,
        };
        let scale = DpiScale { dpi: 96 };
        let click_pt = Some(POINT { x: 100, y: 20 });
        let (x, y, w, h, dock_top) = get_flyout_geometry(&mi, &scale, click_pt);

        assert!(dock_top);
        assert_eq!(w, 420);
        assert_eq!(h, 580);
        assert_eq!(x, 0 + 12);
        assert_eq!(y, 0 + 12);
    }

    #[test]
    fn test_geometry_negative_monitor_offset() {
        let mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT { left: -1920, top: 0, right: 0, bottom: 1080 },
            rcWork: RECT { left: -1920, top: 0, right: 0, bottom: 1040 },
            dwFlags: 0,
        };
        let scale = DpiScale { dpi: 96 };
        let click_pt = Some(POINT { x: -100, y: 1050 });
        let (x, y, w, h, dock_top) = get_flyout_geometry(&mi, &scale, click_pt);

        assert!(!dock_top);
        assert_eq!(w, 420);
        assert_eq!(h, 580);
        assert_eq!(x, 0 - 420 - 12);
        assert_eq!(y, 1040 - 580 - 12);
    }

    #[test]
    fn test_options_subview_order() {
        let subviews = [
            OptionsSubView::Keybinds,
            OptionsSubView::WindowOps,
            OptionsSubView::Automations,
        ];
        assert_eq!(subviews.len(), 3);
        assert_eq!(subviews[0], OptionsSubView::Keybinds);
        assert_eq!(subviews[1], OptionsSubView::WindowOps);
        assert_eq!(subviews[2], OptionsSubView::Automations);
    }

    #[test]
    fn test_stroke_color_display_names() {
        assert_eq!(get_stroke_color_display("burgundy"), "Burgundy");
        assert_eq!(get_stroke_color_display("crimson"), "Crimson");
        assert_eq!(get_stroke_color_display("emerald"), "Emerald");
        assert_eq!(get_stroke_color_display("cyan"), "Cyan");
        assert_eq!(get_stroke_color_display("blue"), "Royal Blue");
        assert_eq!(get_stroke_color_display("amber"), "Amber Gold");
        assert_eq!(get_stroke_color_display("violet"), "Violet");
        assert_eq!(get_stroke_color_display("white"), "White");
        assert_eq!(get_stroke_color_display("custom_hex"), "custom_hex");
    }
}
