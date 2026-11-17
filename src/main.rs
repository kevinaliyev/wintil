#![windows_subsystem = "windows"]

pub mod automations;
pub mod config;
pub mod flyout;
pub mod hider;
pub mod hotkeys;
pub mod pin;
pub mod registry;
pub mod sentry;
pub mod startup;
pub mod tray;
pub mod version;
pub mod window;

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::core::w;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    ChangeWindowMessageFilterEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    EnumWindows, GetMessageW, IsWindowVisible, KillTimer, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetTimer, TranslateMessage, MSG, MSGFLT_ALLOW,
    WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_ENDSESSION, WM_HOTKEY,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_QUERYENDSESSION, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

use crate::config::AppConfig;
use crate::flyout::{invalidate_flyout, toggle_flyout};
use crate::hider::daemon::AutoHiderService;
use crate::hider::HiderStore;
use crate::hotkeys::{HotkeyAction, HotkeyManager};
use crate::pin::hud::show_pin_hud;
use crate::pin::stroke::StrokeManager;
use crate::pin::PinStore;
use crate::tray::{TrayManager, WM_APP_TRAY};
use crate::window::fullscreen::FakeFullscreenStore;
use crate::window::{
    close_window, get_foreground_window, kill_active_process, kill_all_active_by_exe,
    minimize, toggle_maximize,
};

static RESTORE_ON_EXIT: AtomicBool = AtomicBool::new(true);
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static TRAY_RETRY_COUNT: AtomicU32 = AtomicU32::new(0);
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static WAS_INACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_INACTIVE_SECS: AtomicU32 = AtomicU32::new(0);

const TIMER_TRAY_RETRY: usize = 1001;
const TIMER_CONFIG_WATCHER: usize = 1002;
const MAX_TRAY_RETRIES: u32 = 10;

const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const DEVICE_NOTIFY_WINDOW_HANDLE: u32 = 0x00000000;

#[repr(C)]
#[allow(non_snake_case)]
struct LASTINPUTINFO {
    cbSize: u32,
    dwTime: u32,
}

extern "system" {
    fn GetLastInputInfo(plii: *mut LASTINPUTINFO) -> windows::Win32::Foundation::BOOL;
    fn GetTickCount() -> u32;
    fn RegisterSuspendResumeNotification(hrecipient: HWND, flags: u32) -> isize;
    fn UnregisterSuspendResumeNotification(handle: isize) -> windows::Win32::Foundation::BOOL;
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct AppState {
    config: AppConfig,
    config_path: PathBuf,
    last_config_modified: std::time::SystemTime,
    store: HiderStore,
    pin_store: PinStore,
    fake_fullscreen: FakeFullscreenStore,
    stroke_mgr: StrokeManager,
    hotkeys: HotkeyManager,
    tray: TrayManager,
    auto_hide_service: Option<AutoHiderService>,
    automations: crate::automations::AutomationManager,
}

#[derive(Clone)]
pub struct RunningAppInfo {
    pub title: String,
    pub display_name: String,
    pub target_rule: String,
    pub is_already_ruled: bool,
}

#[derive(Clone)]
pub struct FlyoutSnapshot {
    pub hidden_windows: Vec<HiddenWindowInfo>,
    pub pinned_windows: Vec<PinnedWindowInfo>,
    pub stroke_enabled: bool,
    pub stroke_thickness: i32,
    pub stroke_color: String,
    pub hud_enabled: bool,
    pub config_path: String,
    pub single_instance: bool,
    pub auto_hide_executables: Vec<String>,
    pub running_apps: Vec<RunningAppInfo>,
    pub automations: Vec<AutomationInfo>,
    pub services: Vec<ServiceSnapshotInfo>,
    pub subservices: Vec<SubserviceSnapshotInfo>,
    pub startup_task: startup::StartupTaskStatus,
    pub sentry_logging: bool,
    pub sentry_respawn: bool,
    pub sentry_tripped: bool,
    pub sentry_log_bytes: u64,
    pub sentry_log_errors: usize,
    pub sentry_error_delta: usize,
    pub show_tooltips: bool,
    pub hotkeys: crate::config::HotkeysConfig,
    pub active_hotkey_count: usize,
}

#[derive(Clone)]
pub struct ServiceSnapshotInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub is_active: bool,
    pub details: String,
}

#[derive(Clone)]
pub struct SubserviceSnapshotInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub status: String,
    pub details: String,
    pub is_running: bool,
    pub is_recently_finished: bool,
    pub last_ran_text: String,
    pub action_label: String,
}

#[derive(Clone)]
pub struct AutomationInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub triggers: String,
    pub is_running: bool,
    pub is_recently_finished: bool,
    pub last_ran_text: String,
    pub script_name: String,
    pub is_subservice: bool,
    pub on_startup: bool,
    pub on_wake: bool,
    pub on_inactivity_resume: bool,
    pub inactivity_threshold_secs: u64,
}

#[derive(Clone)]
pub struct HiddenWindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub exe_name: String,
}

#[derive(Clone)]
pub struct PinnedWindowInfo {
    pub hwnd: isize,
    pub title: String,
}

struct RunningAppsContext<'a> {
    current_pid: u32,
    existing_rules: &'a [String],
    apps: Vec<RunningAppInfo>,
}

unsafe extern "system" fn enum_running_apps_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut RunningAppsContext);

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let title = crate::window::get_window_title(hwnd);
    let title_trim = title.trim();
    if title_trim.is_empty()
        || title_trim == "Untitled"
        || title_trim == "Program Manager"
        || title_trim == "Windows Input Experience"
    {
        return BOOL(1);
    }

    if let Some(pid) = crate::window::process::get_window_pid(hwnd) {
        if pid == ctx.current_pid {
            return BOOL(1);
        }

        if let Some(details) = crate::window::process::get_process_details(pid) {
            let exe_lower = details.exe_name.to_ascii_lowercase();
            if exe_lower == "explorer.exe"
                || exe_lower == "wintil.exe"
                || exe_lower == "shellexperiencehost.exe"
                || exe_lower == "searchhost.exe"
                || exe_lower == "applicationframehost.exe"
                || exe_lower == "systemsettings.exe"
                || exe_lower == "textinputhost.exe"
            {
                return BOOL(1);
            }

            // Deduplicate by target_rule
            if !ctx.apps.iter().any(|a| a.target_rule.eq_ignore_ascii_case(&details.target_rule)) {
                let is_already_ruled = ctx.existing_rules.iter().any(|r| {
                    crate::hider::daemon::matches_auto_hide_target(r, &details.exe_name, &details.full_path)
                        || r.eq_ignore_ascii_case(&details.target_rule)
                });

                ctx.apps.push(RunningAppInfo {
                    title: title_trim.to_string(),
                    display_name: details.display_name,
                    target_rule: details.target_rule,
                    is_already_ruled,
                });
            }
        }
    }

    BOOL(1)
}

fn format_automation_triggers(on_startup: bool, on_wake: bool, on_inactivity_resume: bool) -> String {
    let mut trig = Vec::new();
    if on_startup { trig.push("Startup"); }
    if on_wake { trig.push("Wake"); }
    if on_inactivity_resume { trig.push("Inactivity"); }
    if trig.is_empty() {
        "Manual".to_string()
    } else {
        trig.join(", ")
    }
}

pub fn app_get_flyout_snapshot() -> Option<FlyoutSnapshot> {
    APP_STATE.with(|sc| {
        let opt = sc.try_borrow().ok()?;
        let st = opt.as_ref()?;
        let hidden = st
            .store
            .list_hidden()
            .into_iter()
            .map(|h| HiddenWindowInfo {
                hwnd: h.hwnd,
                title: h.title,
                exe_name: h.exe_name,
            })
            .collect();
        st.pin_store.prune_dead();
        st.stroke_mgr.prune_dead();
        let pinned_hwnds = st.pin_store.list_pinned();
        for h in st.stroke_mgr.list_overlay_hwnds() {
            if !pinned_hwnds.contains(&h) {
                st.stroke_mgr.detach_stroke(HWND(h as _));
            }
        }
        let pinned: Vec<PinnedWindowInfo> = pinned_hwnds
            .into_iter()
            .map(|h| PinnedWindowInfo {
                hwnd: h,
                title: crate::window::get_window_title(HWND(h as _)),
            })
            .collect();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let automations = st
            .automations
            .list()
            .iter()
            .map(|a| {
                let triggers = format_automation_triggers(a.on_startup, a.on_wake, a.on_inactivity_resume);
                let status = st.automations.get_status(&a.name);
                let is_running = status.state == crate::automations::AutomationRunState::Running;
                let is_recently_finished = match status.last_finished_unix_ms {
                    Some(fin) => now_ms.saturating_sub(fin) < 4000,
                    None => false,
                };
                let last_ran_text = crate::automations::format_last_ran(status.last_ran_unix_ms);
                let script_name = a.script_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("run.ps1")
                    .to_string();
                AutomationInfo {
                    name: a.name.clone(),
                    description: a.description.clone(),
                    enabled: a.enabled,
                    triggers,
                    is_running,
                    is_recently_finished,
                    last_ran_text,
                    script_name,
                    is_subservice: a.is_subservice,
                    on_startup: a.on_startup,
                    on_wake: a.on_wake,
                    on_inactivity_resume: a.on_inactivity_resume,
                    inactivity_threshold_secs: a.inactivity_threshold_secs,
                }
            })
            .collect();

        // Wintil Services Status (ordered: Key Remapper, Crash Sentry, Automations Engine, Window Pinner, Window Hider)
        let mut services = Vec::new();

        // 1. Key Remapper
        let hotkey_count = st.hotkeys.registered_count();
        services.push(ServiceSnapshotInfo {
            id: "key_remapper".to_string(),
            name: "Key Remapper".to_string(),
            status: if hotkey_count == 1 {
                "1 Hook Active".to_string()
            } else if hotkey_count > 1 {
                format!("{} Hooks Active", hotkey_count)
            } else {
                "Deactivated".to_string()
            },
            is_active: hotkey_count > 0,
            details: if hotkey_count > 0 {
                format!(
                    "Listening on {} global shortcut{}",
                    hotkey_count,
                    if hotkey_count == 1 { "" } else { "s" }
                )
            } else {
                "Global shortcut hooks suspended".to_string()
            },
        });

        // 2. Crash Sentry
        let (sentry_log_bytes, sentry_log_errors) = sentry::get_log_summary();
        let sentry_error_delta = sentry::get_error_delta_since_launch();
        let sentry_tripped = sentry::is_circuit_breaker_tripped();
        let sentry_kb = (sentry_log_bytes + 1023) / 1024;
        let sentry_enabled = st.config.sentry.respawn_on_crash || st.config.sentry.logging_enabled;
        let sentry_status = if sentry_tripped {
            "Tripped".to_string()
        } else if !sentry_enabled {
            "Standby".to_string()
        } else {
            "Active".to_string()
        };
        let sentry_details = if sentry_tripped {
            format!("Circuit breaker tripped • {sentry_log_errors} errors ({sentry_kb} KB)")
        } else if !sentry_enabled {
            "Respawn and diagnostic logging disabled".to_string()
        } else if sentry_log_bytes > 0 {
            format!(
                "Respawn: {} • Logs: {sentry_kb} KB ({sentry_log_errors} errors)",
                if st.config.sentry.respawn_on_crash { "on" } else { "off" }
            )
        } else {
            format!(
                "Respawn: {} • Logs: Clear",
                if st.config.sentry.respawn_on_crash { "on" } else { "off" }
            )
        };
        services.push(ServiceSnapshotInfo {
            id: "sentry".to_string(),
            name: "Crash Sentry".to_string(),
            status: sentry_status,
            is_active: !sentry_tripped && sentry_enabled,
            details: sentry_details,
        });

        // 3. Automations Engine
        let auto_list = st.automations.list();
        let auto_count = auto_list.len();
        let auto_enabled_count = auto_list.iter().filter(|a| a.enabled).count();
        services.push(ServiceSnapshotInfo {
            id: "automations".to_string(),
            name: "Automations Engine".to_string(),
            status: if auto_enabled_count > 0 {
                "Active".to_string()
            } else {
                "Standby".to_string()
            },
            is_active: auto_enabled_count > 0,
            details: if auto_count == 0 {
                "No automation tasks registered".to_string()
            } else {
                format!("{auto_enabled_count} of {auto_count} task{} enabled", if auto_count == 1 { "" } else { "s" })
            },
        });

        // 4. Window Pinner
        let stroke_cfg_enabled = st.config.pin_stroke.enabled;
        let stroke_hooked = st.stroke_mgr.is_hooked();
        let pinned_count = pinned.len();
        let is_pinner_active = pinned_count > 0 || (stroke_cfg_enabled && stroke_hooked);
        let pinner_status = if pinned_count > 0 {
            "Active".to_string()
        } else if stroke_cfg_enabled {
            "Standby".to_string()
        } else {
            "Standby".to_string()
        };
        let pinner_details = format!(
            "{pinned_count} pinned {} • Border stroke: {}",
            if pinned_count == 1 { "window" } else { "windows" },
            if stroke_cfg_enabled {
                format!("{}px ({})", st.config.pin_stroke.thickness, st.config.pin_stroke.color)
            } else {
                "off".to_string()
            }
        );
        services.push(ServiceSnapshotInfo {
            id: "stroke_manager".to_string(),
            name: "Window Pinner".to_string(),
            status: pinner_status,
            is_active: is_pinner_active,
            details: pinner_details,
        });

        // 5. Window Hider
        let hider_active = st.auto_hide_service.is_some();
        let hider_count = st.config.hider.auto_hide_executables.len();
        let hidden_count = st.store.list_hidden().len();
        services.push(ServiceSnapshotInfo {
            id: "auto_hider".to_string(),
            name: "Window Hider".to_string(),
            status: if hider_active {
                "Active".to_string()
            } else {
                "Standby".to_string()
            },
            is_active: hider_active,
            details: if hider_active {
                format!(
                    "Watching {} target{} ({} hidden)",
                    hider_count,
                    if hider_count == 1 { "" } else { "s" },
                    hidden_count
                )
            } else if hidden_count > 0 {
                format!("Lazy loaded, starts when needed ({hidden_count} hidden)")
            } else {
                "Lazy loaded, starts when needed.".to_string()
            },
        });

        // Registered Subservices from automations
        let mut subservices = Vec::new();
        for a in st.automations.list() {
            if a.is_subservice {
                let status = st.automations.get_status(&a.name);
                let is_running = status.state == crate::automations::AutomationRunState::Running;
                let is_recently_finished = match status.last_finished_unix_ms {
                    Some(fin) => now_ms.saturating_sub(fin) < 4000,
                    None => false,
                };
                let last_ran_text = crate::automations::format_last_ran(status.last_ran_unix_ms);
                let action_label = if is_running {
                    "Running...".to_string()
                } else if is_recently_finished {
                    "Done ✓".to_string()
                } else {
                    "Run Fix".to_string()
                };

                let trigger_summary = format_automation_triggers(a.on_startup, a.on_wake, a.on_inactivity_resume);
                let ready_status = if trigger_summary == "Manual" {
                    "Ready (Manual)".to_string()
                } else {
                    format!("Ready (triggers: {})", trigger_summary)
                };

                subservices.push(SubserviceSnapshotInfo {
                    id: a.name.clone(),
                    name: a.name.clone(),
                    display_name: if a.subservice_name.is_empty() {
                        a.name.clone()
                    } else {
                        a.subservice_name.clone()
                    },
                    status: if !a.enabled {
                        "Disabled".to_string()
                    } else if is_running {
                        "Applying fix...".to_string()
                    } else if is_recently_finished {
                        "Fix applied successfully".to_string()
                    } else {
                        ready_status
                    },
                    details: a.description.clone(),
                    is_running,
                    is_recently_finished,
                    last_ran_text,
                    action_label,
                });
            }
        }

        let mut apps_ctx = RunningAppsContext {
            current_pid: std::process::id(),
            existing_rules: &st.config.hider.auto_hide_executables,
            apps: Vec::new(),
        };
        unsafe {
            let _ = EnumWindows(
                Some(enum_running_apps_proc),
                LPARAM(&mut apps_ctx as *mut _ as isize),
            );
        }
        let running_apps = apps_ctx.apps;

        Some(FlyoutSnapshot {
            hidden_windows: hidden,
            pinned_windows: pinned,
            stroke_enabled: st.config.pin_stroke.enabled,
            stroke_thickness: st.config.pin_stroke.thickness,
            stroke_color: st.config.pin_stroke.color.clone(),
            hud_enabled: st.config.general.show_pin_hud,
            config_path: st.config_path.to_string_lossy().to_string(),
            single_instance: st.config.general.single_instance,
            auto_hide_executables: st.config.hider.auto_hide_executables.clone(),
            running_apps,
            automations,
            services,
            subservices,
            startup_task: startup::query_task_status(),
            sentry_logging: st.config.sentry.logging_enabled,
            sentry_respawn: st.config.sentry.respawn_on_crash,
            sentry_tripped,
            sentry_log_bytes,
            sentry_log_errors,
            sentry_error_delta,
            show_tooltips: st.config.general.show_tooltips,
            hotkeys: st.config.hotkeys.clone(),
            active_hotkey_count: hotkey_count,
        })
    })
}

pub fn app_unpin_hwnd(h: isize) {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.pin_store.unpin_by_hwnd(h, st.config.general.pin_title_tag);
                st.stroke_mgr.detach_stroke(HWND(h as _));
            }
        }
    });
    invalidate_flyout();
}

pub fn app_unpin_all() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                for h in st.pin_store.list_pinned() {
                    st.pin_store.unpin_by_hwnd(h, st.config.general.pin_title_tag);
                    st.stroke_mgr.detach_stroke(HWND(h as _));
                }
            }
        }
    });
    invalidate_flyout();
}

fn check_autohide_service_lifecycle(st: &mut AppState) {
    if st.config.hider.auto_hide_executables.is_empty() {
        st.auto_hide_service = None;
    }
}

pub fn app_restore_hwnd(h: isize) {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.store.restore_by_hwnd(h);
                check_autohide_service_lifecycle(st);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_restore_last() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.store.restore_last();
                check_autohide_service_lifecycle(st);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_restore_all() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.store.restore_all();
                check_autohide_service_lifecycle(st);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_flush_modifiers() {
    crate::hotkeys::audit::clean_release_held_modifiers();
    crate::hotkeys::audit::log_audit_event("MANUAL_FLUSH", "Manual keyboard flush triggered from UI");
    invalidate_flyout();
}

pub fn app_clear_logs() {
    sentry::clear_logs();
    invalidate_flyout();
}

pub fn app_toggle_sentry_logging() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(ref mut st) = *opt {
                st.config.sentry.logging_enabled = !st.config.sentry.logging_enabled;
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_toggle_sentry_respawn() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(ref mut st) = *opt {
                st.config.sentry.respawn_on_crash = !st.config.sentry.respawn_on_crash;
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_open_logs() {
    let logs_dir = crate::config::get_logs_dir();
    let logs_str = logs_dir.to_string_lossy();
    let wide: Vec<u16> = logs_str.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(
            HWND(std::ptr::null_mut()),
            windows::core::w!("open"),
            windows::core::PCWSTR(wide.as_ptr()),
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
        );
    }
    sentry::log_info("main", &format!("Opened logs directory: {}", logs_dir.display()));
}

pub fn app_toggle_tooltips() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(ref mut st) = *opt {
                st.config.general.show_tooltips = !st.config.general.show_tooltips;
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_clear_hidden() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.store.clear();
                check_autohide_service_lifecycle(st);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_toggle_stroke() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.config.pin_stroke.enabled = !st.config.pin_stroke.enabled;
                st.stroke_mgr.update_config(st.config.pin_stroke.clone());
                if st.config.pin_stroke.enabled {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.attach_stroke(HWND(h as _));
                    }
                } else {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.detach_stroke(HWND(h as _));
                    }
                }
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub const STROKE_COLOR_PRESETS: &[(&str, &str)] = &[
    ("burgundy", "Burgundy"),
    ("crimson", "Crimson"),
    ("emerald", "Emerald"),
    ("cyan", "Cyan"),
    ("blue", "Royal Blue"),
    ("amber", "Amber Gold"),
    ("violet", "Violet"),
    ("white", "White"),
];

pub fn app_cycle_stroke_color() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                let current_col = st.config.pin_stroke.color.to_lowercase();
                let current_idx = STROKE_COLOR_PRESETS
                    .iter()
                    .position(|(key, _)| current_col == *key)
                    .unwrap_or(0);
                let next_idx = (current_idx + 1) % STROKE_COLOR_PRESETS.len();
                let (next_key, _) = STROKE_COLOR_PRESETS[next_idx];
                st.config.pin_stroke.color = next_key.to_string();
                st.stroke_mgr.update_config(st.config.pin_stroke.clone());
                if st.config.pin_stroke.enabled {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.attach_stroke(HWND(h as _));
                    }
                }
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_step_stroke_thickness(delta: i32) {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                let current = st.config.pin_stroke.thickness;
                let new_thick = (current + delta).clamp(1, 8);
                st.config.pin_stroke.thickness = new_thick;
                st.stroke_mgr.update_config(st.config.pin_stroke.clone());
                if st.config.pin_stroke.enabled {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.attach_stroke(HWND(h as _));
                    }
                }
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_toggle_hud() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.config.general.show_pin_hud = !st.config.general.show_pin_hud;
                let _ = st.config.save_to_path(&st.config_path);
                if let Ok(meta) = std::fs::metadata(&st.config_path) {
                    if let Ok(mod_time) = meta.modified() {
                        st.last_config_modified = mod_time;
                    }
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_test_hud() {
    APP_STATE.with(|sc| {
        if let Ok(opt) = sc.try_borrow() {
            if let Some(st) = opt.as_ref() {
                let target = crate::flyout::get_flyout_hwnd().unwrap_or(HWND(std::ptr::null_mut()));
                show_pin_hud(target, true, 800, &st.config.pin_hud);
            }
        }
    });
}

pub fn app_open_config() {
    APP_STATE.with(|sc| {
        if let Ok(opt) = sc.try_borrow() {
            if let Some(st) = opt.as_ref() {
                let _ = std::process::Command::new("notepad.exe")
                    .arg(&st.config_path)
                    .spawn();
            }
        }
    });
}

pub fn app_reload_config() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                let (new_cfg, _) = AppConfig::load();
                st.config = new_cfg;
                st.hotkeys.register_all(&st.config.hotkeys);
                st.stroke_mgr.update_config(st.config.pin_stroke.clone());
                if st.config.pin_stroke.enabled {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.attach_stroke(HWND(h as _));
                    }
                } else {
                    for h in st.pin_store.list_pinned() {
                        st.stroke_mgr.detach_stroke(HWND(h as _));
                    }
                }
                if !st.config.hider.auto_hide_executables.is_empty() {
                    st.auto_hide_service = Some(AutoHiderService::start(
                        st.store.clone(),
                        st.config.hider.auto_hide_executables.clone(),
                        st.config.hider.service_interval_ms,
                    ));
                } else {
                    st.auto_hide_service = None;
                }
                let hwnd = HWND(MAIN_HWND.load(Ordering::Relaxed) as _);
                show_pin_hud(hwnd, true, 800, &st.config.pin_hud);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_run_automation(name: &str) {
    let name_owned = name.to_string();
    APP_STATE.with(|sc| {
        if let Ok(opt) = sc.try_borrow() {
            if let Some(st) = opt.as_ref() {
                st.automations.run_by_name(&name_owned);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_get_automation_for_edit(name: &str) -> Option<crate::automations::AutomationEditState> {
    APP_STATE.with(|sc| {
        let opt = sc.try_borrow().ok()?;
        let st = opt.as_ref()?;
        let auto = st.automations.list().iter().find(|a| a.name.eq_ignore_ascii_case(name))?;
        let script_file = auto.script_path.file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("run.ps1")
            .to_string();
        Some(crate::automations::AutomationEditState {
            is_new: false,
            original_name: auto.name.clone(),
            name: auto.name.clone(),
            description: auto.description.clone(),
            enabled: auto.enabled,
            on_startup: auto.on_startup,
            on_wake: auto.on_wake,
            on_inactivity_resume: auto.on_inactivity_resume,
            inactivity_threshold_secs: auto.inactivity_threshold_secs,
            is_subservice: auto.is_subservice,
            subservice_name: auto.subservice_name.clone(),
            script: script_file,
            dir_path: auto.dir_path.clone(),
            source_script_path: None,
            only_manual: !auto.on_startup && !auto.on_wake && !auto.on_inactivity_resume,
        })
    })
}

pub fn app_check_automation_conflict(edit: &crate::automations::AutomationEditState) -> Option<String> {
    APP_STATE.with(|sc| {
        let opt = sc.try_borrow().ok()?;
        let st = opt.as_ref()?;
        st.automations.find_automation_conflict(edit)
    })
}

pub fn app_save_automation(edit: &crate::automations::AutomationEditState) -> Result<(), String> {
    let res = APP_STATE.with(|sc| {
        let mut opt = sc.borrow_mut();
        let st = opt.as_mut().ok_or_else(|| "App state unavailable".to_string())?;
        st.automations.save_automation(edit)
    });
    invalidate_flyout();
    res
}

pub fn app_delete_automation(name: &str) -> Result<(), String> {
    let res = APP_STATE.with(|sc| {
        let mut opt = sc.borrow_mut();
        let st = opt.as_mut().ok_or_else(|| "App state unavailable".to_string())?;
        st.automations.delete_automation(name)
    });
    invalidate_flyout();
    res
}

pub fn app_open_automation_folder(dir: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(dir)
        .spawn();
}

pub fn app_open_automation_toml(manifest_path: &std::path::Path) {
    let _ = std::process::Command::new("notepad.exe")
        .arg(manifest_path)
        .spawn();
}

pub fn app_toggle_auto_hider() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                if st.auto_hide_service.is_some() {
                    st.auto_hide_service = None;
                } else {
                    st.auto_hide_service = Some(AutoHiderService::start(
                        st.store.clone(),
                        st.config.hider.auto_hide_executables.clone(),
                        st.config.hider.service_interval_ms,
                    ));
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_add_auto_hide_rule(exe: String) {
    let clean = exe.trim().to_string();
    if clean.is_empty() {
        return;
    }
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                if !st.config.hider.auto_hide_executables.iter().any(|e| e.eq_ignore_ascii_case(&clean)) {
                    st.config.hider.auto_hide_executables.push(clean.clone());
                    let _ = st.config.save_to_path(&st.config_path);
                    let targets = st.config.hider.auto_hide_executables.clone();
                    st.auto_hide_service = Some(AutoHiderService::start(
                        st.store.clone(),
                        targets,
                        st.config.hider.service_interval_ms,
                    ));
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_remove_auto_hide_rule(exe: &str) {
    let target = exe.trim();
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.config.hider.auto_hide_executables.retain(|e| !e.eq_ignore_ascii_case(target));
                let _ = st.config.save_to_path(&st.config_path);
                let targets = st.config.hider.auto_hide_executables.clone();
                if targets.is_empty() {
                    st.auto_hide_service = None;
                } else {
                    st.auto_hide_service = Some(AutoHiderService::start(
                        st.store.clone(),
                        targets,
                        st.config.hider.service_interval_ms,
                    ));
                }
            }
        }
    });
    invalidate_flyout();
}

pub fn app_suspend_hotkeys() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.hotkeys.unregister_all();
            }
        }
    });
    invalidate_flyout();
}

pub fn app_resume_hotkeys() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.hotkeys.register_all(&st.config.hotkeys);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_check_hotkey_conflict(action_id: &str, candidate_binding: &str) -> Option<&'static str> {
    APP_STATE.with(|sc| {
        let opt = sc.try_borrow().ok()?;
        let st = opt.as_ref()?;
        crate::config::keys::find_hotkey_conflict(&st.config.hotkeys, action_id, candidate_binding)
    })
}

pub fn app_update_hotkey(action_id: &str, new_binding: &str) -> bool {
    let parsed = crate::config::keys::parse_keybind(new_binding);
    if parsed.is_none() {
        app_resume_hotkeys();
        return false;
    }

    let mut updated = false;
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                if crate::config::keys::find_hotkey_conflict(&st.config.hotkeys, action_id, new_binding).is_some() {
                    return;
                }
                match action_id {
                    "maximize_toggle" => st.config.hotkeys.maximize_toggle = new_binding.to_string(),
                    "fake_fullscreen" => st.config.hotkeys.fake_fullscreen = new_binding.to_string(),
                    "minimize_active" => st.config.hotkeys.minimize_active = new_binding.to_string(),
                    "close_active" => st.config.hotkeys.close_active = new_binding.to_string(),
                    "kill_active" => st.config.hotkeys.kill_active = new_binding.to_string(),
                    "kill_all_by_exe" => st.config.hotkeys.kill_all_by_exe = new_binding.to_string(),
                    "hide_to_tray" => st.config.hotkeys.hide_to_tray = new_binding.to_string(),
                    "toggle_pin" => st.config.hotkeys.toggle_pin = new_binding.to_string(),
                    "restore_last" => st.config.hotkeys.restore_last = new_binding.to_string(),
                    "restore_all" => st.config.hotkeys.restore_all = new_binding.to_string(),
                    "unstick_keys" => st.config.hotkeys.unstick_keys = new_binding.to_string(),
                    _ => {
                        st.hotkeys.register_all(&st.config.hotkeys);
                        return;
                    }
                }
                let _ = st.config.save_to_path(&st.config_path);
                st.hotkeys.register_all(&st.config.hotkeys);
                updated = true;
            }
        }
    });
    invalidate_flyout();
    updated
}

pub fn app_reset_default_hotkeys() {
    APP_STATE.with(|sc| {
        if let Ok(mut opt) = sc.try_borrow_mut() {
            if let Some(st) = opt.as_mut() {
                st.config.hotkeys = crate::config::HotkeysConfig::default();
                let _ = st.config.save_to_path(&st.config_path);
                st.hotkeys.register_all(&st.config.hotkeys);
            }
        }
    });
    invalidate_flyout();
}

pub fn app_exit() {
    let hwnd = HWND(MAIN_HWND.load(Ordering::Relaxed) as _);
    unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let taskbar_created = WM_TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        APP_STATE.with(|state_cell| {
            if let Ok(mut opt) = state_cell.try_borrow_mut() {
                if let Some(state) = opt.as_mut() {
                    state.tray.add(&state.config.tray.tooltip);
                }
            }
        });
        return LRESULT(0);
    }

    match msg {
        WM_TIMER => {
            if wparam.0 == TIMER_TRAY_RETRY {
                let mut added = false;
                APP_STATE.with(|state_cell| {
                    if let Ok(mut opt) = state_cell.try_borrow_mut() {
                        if let Some(state) = opt.as_mut() {
                            if state.tray.add(&state.config.tray.tooltip) {
                                added = true;
                            }
                        }
                    }
                });

                let count = TRAY_RETRY_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                if added || count >= MAX_TRAY_RETRIES {
                    unsafe {
                        let _ = KillTimer(hwnd, TIMER_TRAY_RETRY);
                    }
                }
            } else if wparam.0 == TIMER_CONFIG_WATCHER {
                APP_STATE.with(|state_cell| {
                    if let Ok(mut opt) = state_cell.try_borrow_mut() {
                        if let Some(state) = opt.as_mut() {
                            if let Ok(meta) = std::fs::metadata(&state.config_path) {
                                if let Ok(mod_time) = meta.modified() {
                                    if mod_time > state.last_config_modified {
                                        state.last_config_modified = mod_time;
                                        let (new_cfg, _) = AppConfig::load();
                                        state.config = new_cfg;
                                        state.hotkeys.register_all(&state.config.hotkeys);
                                        state.stroke_mgr.update_config(state.config.pin_stroke.clone());
                                        if state.config.pin_stroke.enabled {
                                            for h in state.pin_store.list_pinned() {
                                                state.stroke_mgr.attach_stroke(HWND(h as _));
                                            }
                                        } else {
                                            for h in state.pin_store.list_pinned() {
                                                state.stroke_mgr.detach_stroke(HWND(h as _));
                                            }
                                        }
                                        if !state.config.hider.auto_hide_executables.is_empty() {
                                            state.auto_hide_service = Some(AutoHiderService::start(
                                                state.store.clone(),
                                                state.config.hider.auto_hide_executables.clone(),
                                                state.config.hider.service_interval_ms,
                                            ));
                                        } else {
                                            state.auto_hide_service = None;
                                        }
                                        invalidate_flyout();
                                    }
                                }
                            }

                            // 2. Inactivity tracking for automations
                            let mut lii = LASTINPUTINFO {
                                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                                dwTime: 0,
                            };
                            if unsafe { GetLastInputInfo(&mut lii) }.as_bool() {
                                let tick = unsafe { GetTickCount() };
                                let idle_ms = tick.wrapping_sub(lii.dwTime);
                                let idle_secs = (idle_ms / 1000) as u32;

                                let was_inactive = WAS_INACTIVE.load(Ordering::Relaxed);
                                if idle_secs >= 60 {
                                    if !was_inactive {
                                        WAS_INACTIVE.store(true, Ordering::Relaxed);
                                        LAST_INACTIVE_SECS.store(idle_secs, Ordering::Relaxed);
                                    } else {
                                        LAST_INACTIVE_SECS.store(idle_secs, Ordering::Relaxed);
                                    }
                                } else if was_inactive {
                                    // User just returned from inactivity!
                                    // Note: Do NOT inject synthetic key events on resume!
                                    WAS_INACTIVE.store(false, Ordering::Relaxed);
                                    let total_idle = LAST_INACTIVE_SECS.swap(0, Ordering::Relaxed);
                                    crate::hotkeys::audit::log_audit_event(
                                        "INACTIVITY_RESUME",
                                        &format!("User resumed after {}s idle", total_idle),
                                    );
                                    state.automations.trigger_inactivity_resume(total_idle as u64);
                                }
                            }
                        }
                    }
                });
            }
            LRESULT(0)
        }

        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMRESUMEAUTOMATIC || wparam.0 == PBT_APMRESUMESUSPEND {
                crate::hotkeys::audit::clean_release_held_modifiers();
                crate::hotkeys::audit::log_audit_event(
                    "POWER_RESUME",
                    "System resumed from sleep / Modern Standby",
                );
                APP_STATE.with(|state_cell| {
                    if let Ok(opt) = state_cell.try_borrow() {
                        if let Some(state) = opt.as_ref() {
                            state.automations.trigger_wake();
                        }
                    }
                });
            }
            LRESULT(1)
        }

        WM_HOTKEY => {
            let action_id = wparam.0 as i32;
            if let Some(action) = HotkeyAction::from_id(action_id) {
                APP_STATE.with(|state_cell| {
                    if let Ok(mut opt) = state_cell.try_borrow_mut() {
                        if let Some(state) = opt.as_mut() {
                            handle_hotkey(action, state);
                        }
                    }
                });
                invalidate_flyout();
            }
            LRESULT(0)
        }

        WM_APP_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_LBUTTONDOWN {
                // If the flyout is currently open or closing, dismiss it on mouse down
                // so subsequent mouse up on the tray does not rehydrate the UI
                if crate::flyout::is_flyout_visible() || crate::flyout::is_flyout_closing() {
                    crate::flyout::record_flyout_dismiss();
                    crate::flyout::hide_flyout();
                }
            } else if event == WM_LBUTTONUP {
                // Left click toggles the Popout Flyout GUI
                toggle_flyout();
            }
            LRESULT(0)
        }

        WM_CLOSE | WM_QUERYENDSESSION => {
            sentry::mark_clean_shutdown();
            APP_STATE.with(|state_cell| {
                if let Ok(mut opt) = state_cell.try_borrow_mut() {
                    if let Some(state) = opt.as_mut() {
                        if state.config.hider.restore_all_on_exit {
                            state.store.restore_all();
                        }
                        state.fake_fullscreen.restore_all();
                        for h in state.pin_store.list_pinned() {
                            if state.config.general.pin_title_tag {
                                state.pin_store.unpin_by_hwnd(h, true);
                            }
                            state.stroke_mgr.detach_stroke(HWND(h as _));
                        }
                        state.tray.remove();
                    }
                }
            });
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(1)
        }

        WM_DESTROY | WM_ENDSESSION => {
            sentry::mark_clean_shutdown();
            APP_STATE.with(|state_cell| {
                if let Ok(mut opt) = state_cell.try_borrow_mut() {
                    if let Some(state) = opt.as_mut() {
                        if state.config.hider.restore_all_on_exit {
                            state.store.restore_all();
                        }
                        state.fake_fullscreen.restore_all();
                        for h in state.pin_store.list_pinned() {
                            if state.config.general.pin_title_tag {
                                state.pin_store.unpin_by_hwnd(h, true);
                            }
                            state.stroke_mgr.detach_stroke(HWND(h as _));
                        }
                        state.tray.remove();
                    }
                }
            });
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }

        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn handle_hotkey(action: HotkeyAction, state: &mut AppState) {
    crate::hotkeys::audit::log_audit_event("HOTKEY_TRIGGER", &format!("{:?}", action));

    match action {
        HotkeyAction::ToggleMaximize => {
            if let Some(fg) = get_foreground_window() {
                state.fake_fullscreen.exit(fg);
                toggle_maximize(fg);
            }
        }
        HotkeyAction::FakeFullscreen => {
            if let Some(fg) = get_foreground_window() {
                state.fake_fullscreen.toggle(fg);
            }
        }
        HotkeyAction::MinimizeActive => {
            if let Some(fg) = get_foreground_window() {
                minimize(fg);
            }
        }
        HotkeyAction::CloseActive => {
            if let Some(fg) = get_foreground_window() {
                close_window(fg);
                crate::hotkeys::audit::clean_release_held_modifiers();
            }
        }
        HotkeyAction::KillActive => {
            if let Some(fg) = get_foreground_window() {
                let _ = kill_active_process(fg);
                crate::hotkeys::audit::clean_release_held_modifiers();
            }
        }
        HotkeyAction::KillAllByExe => {
            if let Some(fg) = get_foreground_window() {
                let _ = kill_all_active_by_exe(fg);
                crate::hotkeys::audit::clean_release_held_modifiers();
            }
        }
        HotkeyAction::HideToTray => {
            if let Some(target) = crate::window::get_foreground_window() {
                state.store.hide_window(target);
                crate::hotkeys::audit::clean_release_held_modifiers();
            }
        }
        HotkeyAction::TogglePin => {
            if let Some(fg) = get_foreground_window() {
                if let Ok(is_pinned) = state.pin_store.toggle_pin(fg, state.config.general.pin_title_tag) {
                    if is_pinned {
                        state.stroke_mgr.attach_stroke(fg);
                    } else {
                        state.stroke_mgr.detach_stroke(fg);
                    }

                    if state.config.general.show_pin_hud {
                        show_pin_hud(
                            fg,
                            is_pinned,
                            state.config.general.notification_duration_ms,
                            &state.config.pin_hud,
                        );
                    }
                }
            }
        }
        HotkeyAction::RestoreLast => {
            state.store.restore_last();
            check_autohide_service_lifecycle(state);
        }
        HotkeyAction::RestoreAll => {
            state.store.restore_all();
            check_autohide_service_lifecycle(state);
        }
        HotkeyAction::UnstickKeys => {
            crate::hotkeys::audit::universal_unstick_all_keys();
        }
    }
}

fn main() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 {
        match args[1].as_str() {
            "--version" | "-v" => {
                println!("{}", crate::version::banner());
                return;
            }
            "--help" | "-h" => {
                println!("{}", crate::version::banner());
                println!("Usage: wintil.exe [OPTIONS]\n");
                println!("Options:");
                println!("  -v, --version        Display version information and exit");
                println!("  -h, --help           Display this help message and exit");
                println!("  --audit-keys         Run interactive live keyboard telemetry and stuck-key monitor");
                println!("  --flush-keys         Forcibly release and sanitize all modifier keys");
                println!("  --list-automations   List discovered hardware and wake automations");
                println!("  --run-automation <N> Manually execute a specific automation by name");
                println!("  --sentry <PID>       Internal watchdog supervisor mode");
                return;
            }
            "--audit" | "--audit-keys" => {
                crate::hotkeys::audit::run_cli_audit_monitor();
                return;
            }
            "--flush-keys" | "--flush" => {
                crate::hotkeys::audit::clean_release_held_modifiers();
                println!("All keyboard modifiers flushed and sanitized successfully.");
                return;
            }
            "--list-automations" | "--automations" => {
                let mgr = crate::automations::AutomationManager::load();
                println!("================================================================================");
                println!("                        WINTIL AUTOMATIONS MANAGER (v{})", crate::version::APP_VERSION);
                println!("================================================================================");
                let list = mgr.list();
                if list.is_empty() {
                    println!("No automations discovered.");
                    println!("Place manifests in 'automations/<name>/automation.toml'.");
                } else {
                    for (i, a) in list.iter().enumerate() {
                        let mut trig: Vec<String> = Vec::new();
                        if a.on_startup { trig.push("Startup".to_string()); }
                        if a.on_wake { trig.push("Wake".to_string()); }
                        if a.on_inactivity_resume {
                            trig.push(format!("Inactivity ({}s)", a.inactivity_threshold_secs));
                        }
                        println!("{}. [{}] {}", i + 1, a.name, if a.enabled { "(Enabled)" } else { "(Disabled)" });
                        println!("   Description: {}", a.description);
                        println!("   Triggers:    {}", trig.join(", "));
                        println!("   Script:      {:?}", a.script_path);
                    }
                }
                return;
            }
            "--run-automation" => {
                if args.len() >= 3 {
                    let mgr = crate::automations::AutomationManager::load();
                    let name = &args[2];
                    if mgr.run_by_name(name) {
                        println!("Automation '{}' launched in background.", name);
                    } else {
                        eprintln!("Error: Automation '{}' not found or script missing.", name);
                    }
                } else {
                    eprintln!("Usage: wintil.exe --run-automation <NAME>");
                }
                return;
            }
            _ => {}
        }
    }

    if args.len() >= 3 && args[1] == "--sentry" {
        if let Ok(master_pid) = args[2].parse::<u32>() {
            sentry::run_sentry(master_pid);
            return;
        }
    }

    let (config, config_path) = AppConfig::load();

    // Single-instance enforcement
    let mut _mutex_handle: Option<HANDLE> = None;
    if config.general.single_instance {
        unsafe {
            let handle = CreateMutexW(
                None,
                true,
                w!("Global\\WintilSingleInstanceMutex"),
            );
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return;
            }
            if let Ok(h) = handle {
                _mutex_handle = Some(h);
            }
        }
    }

    let my_pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
    sentry::init_master_journal(my_pid);
    sentry::spawn_sentry(my_pid);

    let store = HiderStore::new();
    let pin_store = PinStore::new();
    let stroke_mgr = StrokeManager::new(config.pin_stroke.clone());

    // Set panic hook to restore hidden windows in case of crash and log panic details
    let store_failsafe = store.clone();
    std::panic::set_hook(Box::new(move |panic_info| {
        sentry::log_event(
            "PANIC",
            "main",
            &panic_info.to_string(),
            None,
        );
        if RESTORE_ON_EXIT.load(Ordering::Relaxed) {
            store_failsafe.restore_all();
        }
    }));

    sentry::init_launch_error_baseline();
    startup::ensure_persistent_task_if_needed();

    // Register Win32 message window class
    let class_name = w!("WintilMessageWindowClass");
    let wnd_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: class_name,
        ..Default::default()
    };

    unsafe {
        RegisterClassW(&wnd_class);

        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            w!("WintilMessageWindow"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND(std::ptr::null_mut()),
            None,
            None,
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);

        // Register TaskbarCreated message broadcast by Explorer on startup or restart
        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));
        WM_TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);

        // Allow TaskbarCreated through Windows UIPI filter so elevated instances receive it from Medium-integrity Explorer
        if taskbar_created != 0 {
            let _ = ChangeWindowMessageFilterEx(hwnd, taskbar_created, MSGFLT_ALLOW, None);
        }

        // Initialize hotkey manager and register keybinds
        let mut hotkeys = HotkeyManager::new(hwnd);
        hotkeys.register_all(&config.hotkeys);

        // Initialize tray icon manager
        let tray = TrayManager::new(hwnd, &config.tray.icon_path);
        let tray_added = tray.add(&config.tray.tooltip);

        // If Shell_NotifyIcon failed at initial startup, retry for up to 10 seconds
        if !tray_added {
            TRAY_RETRY_COUNT.store(0, Ordering::Relaxed);
            let _ = SetTimer(hwnd, TIMER_TRAY_RETRY, 1000, None);
        }

        // Start auto-hide background watcher only if rules are configured (lazy-loaded)
        let auto_hide_service = if !config.hider.auto_hide_executables.is_empty() {
            Some(AutoHiderService::start(
                store.clone(),
                config.hider.auto_hide_executables.clone(),
                config.hider.service_interval_ms,
            ))
        } else {
            None
        };

        let last_config_modified = std::fs::metadata(&config_path)
            .and_then(|m| m.modified())
            .unwrap_or_else(|_| std::time::SystemTime::now());

        // Background config modification watcher
        let _ = SetTimer(hwnd, TIMER_CONFIG_WATCHER, 1000, None);

        // Register for Modern Standby / Sleep suspend-resume notifications
        let power_notify = RegisterSuspendResumeNotification(hwnd, DEVICE_NOTIFY_WINDOW_HANDLE);

        // Initialize automations manager and execute startup triggers
        let automations = crate::automations::AutomationManager::load();
        automations.trigger_startup();

        // Store state in thread-local storage for window_proc
        APP_STATE.with(|cell| {
            *cell.borrow_mut() = Some(AppState {
                config,
                config_path,
                last_config_modified,
                store,
                pin_store,
                fake_fullscreen: FakeFullscreenStore::new(),
                stroke_mgr,
                hotkeys,
                tray,
                auto_hide_service,
                automations,
            });
        });

        // Standard Win32 Message Pump
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Cleanup state
        APP_STATE.with(|cell| {
            let _ = KillTimer(hwnd, TIMER_TRAY_RETRY);
            let _ = KillTimer(hwnd, TIMER_CONFIG_WATCHER);
            if let Some(mut state) = cell.borrow_mut().take() {
                if let Some(mut s) = state.auto_hide_service.take() {
                    s.stop();
                }
                state.hotkeys.unregister_all();
                state.tray.remove();
            }
        });

        if power_notify != 0 {
            let _ = UnregisterSuspendResumeNotification(power_notify);
        }

        if let Some(h) = _mutex_handle {
            let _ = CloseHandle(h);
        }
    }
}
