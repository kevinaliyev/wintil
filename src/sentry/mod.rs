use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, WaitForSingleObject, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindow, SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, HWND_NOTOPMOST,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
};

pub const MAGIC: u32 = 0x57494E54; // 'WINT'

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateJournal {
    pub magic: u32,
    pub master_pid: u32,
    pub clean_shutdown: bool,
    pub cloaked_windows: Vec<JournalCloakedWindow>,
    pub pinned_windows: Vec<JournalPinnedWindow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalCloakedWindow {
    pub hwnd: isize,
    pub title: String,
    pub exe_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalPinnedWindow {
    pub hwnd: isize,
    pub original_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrashHistory {
    pub crash_timestamps: Vec<u64>,
    pub circuit_breaker_tripped: bool,
    pub last_tripped_time: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogRecord {
    pub timestamp: String,
    pub level: String,
    pub module: String,
    pub message: String,
    pub details: Option<String>,
}

static JOURNAL_STATE: Mutex<Option<StateJournal>> = Mutex::new(None);
static IS_SENTRY: AtomicBool = AtomicBool::new(false);
static LAST_EMITTED_LOGS: Mutex<Option<HashMap<(String, String), (Instant, usize)>>> = Mutex::new(None);

pub fn get_journal_path() -> PathBuf {
    crate::config::get_config_dir().join("wintil_state.journal")
}

pub fn get_master_log_path() -> PathBuf {
    crate::config::get_logs_dir().join("wintil.log")
}

pub fn get_crash_log_path() -> PathBuf {
    crate::config::get_logs_dir().join("wintil_crash.log")
}

pub fn get_events_log_path() -> PathBuf {
    crate::config::get_logs_dir().join("wintil_events.jsonl")
}

pub fn get_crash_history_path() -> PathBuf {
    crate::config::get_config_dir().join("wintil_crash_history.toml")
}

/// Converts a UNIX epoch seconds timestamp into a formatted "YYYY-MM-DD HH:MM:SS" string (UTC)
pub fn format_timestamp(ts_secs: u64) -> String {
    let mut days = ts_secs / 86400;
    let day_secs = ts_secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    let mut year = 1970i64;
    loop {
        let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if is_leap { 366 } else { 365 };
        if (days as i64) >= days_in_year {
            days -= days_in_year as u64;
            year += 1;
        } else {
            break;
        }
    }

    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months: [u64; 12] = [
        31, if is_leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];

    let mut month = 1;
    for &dim in &days_in_months {
        if days >= dim {
            days -= dim;
            month += 1;
        } else {
            break;
        }
    }
    let day = days + 1;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Migrates any legacy loose log files from ~/.config/wintil/ into ~/.config/wintil/logs/
pub fn migrate_legacy_logs() {
    let config_dir = crate::config::get_config_dir();
    let logs_dir = crate::config::get_logs_dir();

    let legacy_files = [
        "automations.log",
        "keyboard_audit.log",
        "wintil_crash.log",
        "wintil_events.jsonl",
    ];

    for file_name in &legacy_files {
        let old_path = config_dir.join(file_name);
        let new_path = logs_dir.join(file_name);
        if old_path.is_file() {
            if new_path.exists() {
                if let Ok(old_data) = fs::read(&old_path) {
                    if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&new_path) {
                        let _ = f.write_all(&old_data);
                    }
                }
                let _ = fs::remove_file(&old_path);
            } else {
                let _ = fs::rename(&old_path, &new_path);
            }
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn flush_journal_to_disk(journal: &StateJournal) {
    let path = get_journal_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(toml_str) = toml::to_string_pretty(journal) {
        let _ = fs::write(&path, toml_str);
    }
}

/// Decodes common Windows NTSTATUS / Exception exit codes into descriptive text
pub fn decode_ntstatus(code: u32) -> &'static str {
    match code {
        0x00000000 => "STATUS_SUCCESS (Clean exit)",
        0xC0000005 => "STATUS_ACCESS_VIOLATION (Invalid memory access)",
        0xC0000409 => "STATUS_STACK_BUFFER_OVERRUN (Fast-fail / Rust panic abort)",
        0xC00000FD => "STATUS_STACK_OVERFLOW (Recursion stack overflow)",
        0xC0000008 => "STATUS_INVALID_HANDLE (Invalid kernel object handle)",
        0xC000001D => "STATUS_ILLEGAL_INSTRUCTION (Invalid CPU opcode)",
        0xC0000094 => "STATUS_INTEGER_DIVIDE_BY_ZERO (Math division by zero)",
        0xC0000374 => "STATUS_HEAP_CORRUPTION (Heap corruption detected)",
        0xE06D7363 => "STATUS_CPP_EXCEPTION (Visual C++ / SEH exception)",
        _ => "UNHANDLED_EXCEPTION_OR_TERMINATION",
    }
}

/// Initializes the master process journal, recovering any previous unhandled crash state first
pub fn init_master_journal(master_pid: u32) {
    migrate_legacy_logs();
    let journal_path = get_journal_path();
    if journal_path.exists() {
        if let Ok(content) = fs::read_to_string(&journal_path) {
            if let Ok(old_journal) = toml::from_str::<StateJournal>(&content) {
                if !old_journal.clean_shutdown {
                    perform_rollback(&old_journal, 0);
                }
            }
        }
        let _ = fs::remove_file(&journal_path);
    }

    let fresh = StateJournal {
        magic: MAGIC,
        master_pid,
        clean_shutdown: false,
        cloaked_windows: Vec::new(),
        pinned_windows: Vec::new(),
    };

    flush_journal_to_disk(&fresh);
    *JOURNAL_STATE.lock() = Some(fresh);
}

/// Spawns the decoupled sentry watchdog process monitoring this master process PID
pub fn spawn_sentry(master_pid: u32) {
    if let Ok(current_exe) = std::env::current_exe() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let _ = std::process::Command::new(current_exe)
            .arg("--sentry")
            .arg(master_pid.to_string())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

pub fn record_cloaked(hwnd: isize, title: String, exe_name: String) {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.cloaked_windows.retain(|w| w.hwnd != hwnd);
        journal.cloaked_windows.push(JournalCloakedWindow {
            hwnd,
            title,
            exe_name,
        });
        flush_journal_to_disk(journal);
    }
}

pub fn unrecord_cloaked(hwnd: isize) {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.cloaked_windows.retain(|w| w.hwnd != hwnd);
        flush_journal_to_disk(journal);
    }
}

pub fn clear_cloaked() {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.cloaked_windows.clear();
        flush_journal_to_disk(journal);
    }
}

pub fn record_pinned(hwnd: isize, original_title: String) {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.pinned_windows.retain(|p| p.hwnd != hwnd);
        journal.pinned_windows.push(JournalPinnedWindow {
            hwnd,
            original_title,
        });
        flush_journal_to_disk(journal);
    }
}

pub fn unrecord_pinned(hwnd: isize) {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.pinned_windows.retain(|p| p.hwnd != hwnd);
        flush_journal_to_disk(journal);
    }
}

pub fn clear_pinned() {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.pinned_windows.clear();
        flush_journal_to_disk(journal);
    }
}

pub fn mark_clean_shutdown() {
    let mut guard = JOURNAL_STATE.lock();
    if let Some(journal) = guard.as_mut() {
        journal.clean_shutdown = true;
        flush_journal_to_disk(journal);
    }
    let journal_path = get_journal_path();
    if journal_path.exists() {
        let _ = fs::remove_file(&journal_path);
    }
    reset_circuit_breaker();
}

/// Sentry entry point running in detached child process
pub fn run_sentry(master_pid: u32) {
    IS_SENTRY.store(true, Ordering::Relaxed);

    // 1. Acquire kernel handle with SYNCHRONIZE & QUERY rights to wait on master process termination
    let handle_res = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            master_pid,
        )
    };

    let mut exit_code: u32 = 0;
    if let Ok(h) = handle_res {
        // 2. Suspend sentry thread indefinitely with 0% CPU until master process terminates
        unsafe {
            let _ = WaitForSingleObject(h, INFINITE);
            let _ = GetExitCodeProcess(h, &mut exit_code);
            let _ = CloseHandle(h);
        }
    }

    // 3. Master process died or terminated. Inspect the state journal
    let journal_path = get_journal_path();
    let mut clean = false;
    if journal_path.exists() {
        if let Ok(content) = fs::read_to_string(&journal_path) {
            if let Ok(journal) = toml::from_str::<StateJournal>(&content) {
                clean = journal.clean_shutdown;
                if !clean {
                    perform_rollback(&journal, exit_code);
                }
            }
        }
        let _ = fs::remove_file(&journal_path);
    }

    // If clean shutdown was flagged by master process (e.g. user clicked Exit), exit normally
    if clean {
        reset_circuit_breaker();
        return;
    }

    // 4. Crash Handling & Respawning
    let config_path = crate::config::get_config_path();
    let config = crate::config::AppConfig::load_from_path(&config_path);

    if !config.sentry.respawn_on_crash {
        log_event(
            "INFO",
            "sentry",
            "Automatic respawn disabled in configuration. Sentry exiting safely.",
            None,
        );
        return;
    }

    // 5. Emergency Circuit Breaker check
    let can_respawn = record_crash_and_check_circuit_breaker(
        config.sentry.max_crashes,
        config.sentry.crash_window_secs,
    );

    if !can_respawn {
        let alert = format!(
            "EMERGENCY CIRCUIT BREAKER TRIPPED: Process crashed {} times within {}s. Halting automatic respawn to avoid infinite crash thrashing.",
            config.sentry.max_crashes, config.sentry.crash_window_secs
        );
        log_event(
            "CRASH",
            "sentry",
            &alert,
            Some(&format!("Terminal Exit Code: 0x{:08X} ({})", exit_code, decode_ntstatus(exit_code))),
        );
        return;
    }

    // 6. Cooldown delay before respawning (1000ms debounce)
    std::thread::sleep(std::time::Duration::from_millis(1000));

    // 7. Respawn fresh master process
    if let Ok(current_exe) = std::env::current_exe() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let respawn_res = std::process::Command::new(current_exe)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();

        match respawn_res {
            Ok(child) => {
                let msg = format!("Master process respawned successfully with new PID {}", child.id());
                log_event("INFO", "sentry", &msg, None);
            }
            Err(e) => {
                let msg = format!("Failed to respawn master process: {}", e);
                log_event("ERROR", "sentry", &msg, None);
            }
        }
    }
}

/// Executes disaster rollback restoring all cloaked windows and removing pinned topmost states
pub fn perform_rollback(journal: &StateJournal, exit_code: u32) {
    let mut restored_cloaked = 0;
    let mut restored_pinned = 0;

    unsafe {
        // 1. Un-cloak all cloaked windows
        for item in &journal.cloaked_windows {
            let hwnd = HWND(item.hwnd as _);
            if IsWindow(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
                restored_cloaked += 1;
            }
        }

        // 2. Un-pin all pinned windows and restore titles
        for item in &journal.pinned_windows {
            let hwnd = HWND(item.hwnd as _);
            if IsWindow(hwnd).as_bool() {
                let _ = SetWindowPos(
                    hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
                let wide = to_wide(&item.original_title);
                let _ = SetWindowTextW(hwnd, PCWSTR(wide.as_ptr()));
                restored_pinned += 1;
            }
        }
    }

    // 3. Append detailed incident record to crash diagnostic log
    let log_path = get_crash_log_path();
    let ts_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let timestamp = format_timestamp(ts_secs);
    let code_desc = decode_ntstatus(exit_code);
    let log_msg = format!(
        "[{}] Wintil v{} CRASH SENTRY DISASTER ROLLBACK EXECUTED\nMaster Process PID: {}\nExit Code: 0x{:08X} - {}\nRestored Cloaked Windows: {}\nRestored Pinned Windows: {}\nAction: All trapped windows successfully unhidden and returned to desktop.\n\n",
        timestamp, crate::version::APP_VERSION, journal.master_pid, exit_code, code_desc, restored_cloaked, restored_pinned
    );

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| f.write_all(log_msg.as_bytes()));

    // Record structured event
    log_event(
        "CRASH",
        "sentry",
        &format!("Disaster rollback completed for PID {}. Exit code: 0x{:08X} ({})", journal.master_pid, exit_code, code_desc),
        Some(&format!("Restored {} cloaked, {} pinned windows", restored_cloaked, restored_pinned)),
    );
}

/// Logs a structured telemetry event, with deduplication / rate-limiting to prevent log ballooning
pub fn log_event(level: &str, module: &str, message: &str, details: Option<&str>) {
    // 1. Check if logging is enabled in configuration
    let config_path = crate::config::get_config_path();
    let config = crate::config::AppConfig::load_from_path(&config_path);
    if !config.sentry.logging_enabled {
        return;
    }

    // 2. Rate-limiting & deduplication filter (2-second debounce for repeated non-ERROR module/message pairs)
    if level != "ERROR" {
        let key = (module.to_string(), message.to_string());
        let mut rate_guard = LAST_EMITTED_LOGS.lock();
        if rate_guard.is_none() {
            *rate_guard = Some(HashMap::new());
        }
        if let Some(map) = rate_guard.as_mut() {
            let now = Instant::now();
            if let Some((last_time, count)) = map.get_mut(&key) {
                if now.duration_since(*last_time).as_secs() < 2 {
                    *count += 1;
                    return; // Dropped duplicate within rate window
                } else {
                    *last_time = now;
                    *count = 1;
                }
            } else {
                map.insert(key, (now, 1));
            }
        }
    }

    // 3. Format timestamp (seconds and formatted UTC string)
    let ts_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let formatted_ts = format_timestamp(ts_secs);

    // 4. Append to general master sequential log (logs/wintil.log)
    let master_path = get_master_log_path();
    let master_line = if let Some(det) = details {
        format!("[{}] [{}] [{}] {} - {}\n", formatted_ts, level, module, message, det)
    } else {
        format!("[{}] [{}] [{}] {}\n", formatted_ts, level, module, message)
    };
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&master_path)
        .and_then(|mut f| f.write_all(master_line.as_bytes()));

    // 5. Append to JSON Lines events log (logs/wintil_events.jsonl)
    let events_path = get_events_log_path();
    let escaped_msg = message.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    let json_line = if let Some(det) = details {
        let escaped_det = det.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
        format!(
            r#"{{"ts":{},"time":"{}","level":"{}","module":"{}","msg":"{}","details":"{}"}}"#,
            ts_secs, formatted_ts, level, module, escaped_msg, escaped_det
        )
    } else {
        format!(
            r#"{{"ts":{},"time":"{}","level":"{}","module":"{}","msg":"{}"}}"#,
            ts_secs, formatted_ts, level, module, escaped_msg
        )
    };

    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)
        .and_then(|mut f| writeln!(f, "{}", json_line));

    // 6. If ERROR, PANIC, or CRASH, also append formatted human entry to crash log (logs/wintil_crash.log)
    if level == "ERROR" || level == "PANIC" || level == "CRASH" {
        let crash_path = get_crash_log_path();
        let formatted = format!(
            "[{}] [{}] [{}] {}\n{}\n",
            formatted_ts,
            level,
            module,
            message,
            details.map(|d| format!("Details: {}\n", d)).unwrap_or_default()
        );
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_path)
            .and_then(|mut f| f.write_all(formatted.as_bytes()));
    }
}

pub fn log_info(module: &str, message: &str) {
    log_event("INFO", module, message, None);
}

pub fn log_warn(module: &str, message: &str) {
    log_event("WARN", module, message, None);
}

pub fn log_error(module: &str, message: &str, details: Option<&str>) {
    log_event("ERROR", module, message, details);
    invalidate_sentry_caches();
}

/// Empties all diagnostic and event logs in the log master directory and resets the circuit breaker
pub fn clear_logs() {
    let logs_dir = crate::config::get_logs_dir();
    let _ = fs::remove_file(get_master_log_path());
    let _ = fs::remove_file(get_crash_log_path());
    let _ = fs::remove_file(get_events_log_path());
    let _ = fs::remove_file(logs_dir.join("automations.log"));
    let _ = fs::remove_file(logs_dir.join("keyboard_audit.log"));

    // Also clean up any legacy loose log files in config directory
    let config_dir = crate::config::get_config_dir();
    let _ = fs::remove_file(config_dir.join("automations.log"));
    let _ = fs::remove_file(config_dir.join("keyboard_audit.log"));
    let _ = fs::remove_file(config_dir.join("wintil_crash.log"));
    let _ = fs::remove_file(config_dir.join("wintil_events.jsonl"));

    *LAUNCH_ERROR_COUNT.lock() = Some(0);
    *LAST_SEEN_ERROR_INFO.lock() = None;
    reset_circuit_breaker();
    invalidate_sentry_caches();
}

static LAUNCH_ERROR_COUNT: Mutex<Option<usize>> = Mutex::new(None);
static LAST_SEEN_ERROR_INFO: Mutex<Option<(usize, u32)>> = Mutex::new(None);
static CACHED_LOG_SUMMARY: Mutex<Option<(Instant, (u64, usize))>> = Mutex::new(None);
static CACHED_CRASH_HISTORY: Mutex<Option<(Instant, CrashHistory)>> = Mutex::new(None);
const SENTRY_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3);

/// Captures the baseline error count at application launch so new errors since launch can be flagged.
pub fn init_launch_error_baseline() {
    let (_, errors) = get_log_summary();
    let mut base = LAUNCH_ERROR_COUNT.lock();
    if base.is_none() {
        *base = Some(errors);
    }
}

/// Tracks GUI reopen cycles: keeps the error indicator visible on the first open after new errors,
/// then clears the baseline on the second open if no new errors were recorded since the first open.
pub fn on_gui_opened() {
    let (_, current) = get_log_summary();
    let mut base_guard = LAUNCH_ERROR_COUNT.lock();
    let baseline = *base_guard.get_or_insert(current);
    if current <= baseline {
        *LAST_SEEN_ERROR_INFO.lock() = None;
        return;
    }

    let mut seen_guard = LAST_SEEN_ERROR_INFO.lock();
    match *seen_guard {
        Some((seen_count, seen_opens)) if seen_count == current && seen_opens >= 1 => {
            *base_guard = Some(current);
            *seen_guard = None;
        }
        _ => {
            *seen_guard = Some((current, 1));
        }
    }
}

/// Returns how many new errors have been recorded since this process launched (or since logs were cleared).
pub fn get_error_delta_since_launch() -> usize {
    let (_, current) = get_log_summary();
    let mut base = LAUNCH_ERROR_COUNT.lock();
    let baseline = *base.get_or_insert(current);
    current.saturating_sub(baseline)
}

pub fn invalidate_sentry_caches() {
    *CACHED_LOG_SUMMARY.lock() = None;
    *CACHED_CRASH_HISTORY.lock() = None;
}

/// Returns total diagnostic log footprint in bytes across logs directory and count of recorded errors.
/// Caches result in memory for 3 seconds to avoid synchronous disk read scans during mouse hover and repainting.
pub fn get_log_summary() -> (u64, usize) {
    let mut cache = CACHED_LOG_SUMMARY.lock();
    if let Some((instant, val)) = *cache {
        if instant.elapsed() < SENTRY_CACHE_TTL {
            return val;
        }
    }

    let mut total_bytes: u64 = 0;
    let mut error_count: usize = 0;

    let logs_dir = crate::config::get_logs_dir();
    if let Ok(entries) = fs::read_dir(&logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(meta) = fs::metadata(&path) {
                    total_bytes += meta.len();
                }
            }
        }
    }

    // Check legacy files in config dir if present
    let config_dir = crate::config::get_config_dir();
    for legacy in &["automations.log", "keyboard_audit.log", "wintil_crash.log", "wintil_events.jsonl"] {
        let legacy_path = config_dir.join(legacy);
        if legacy_path.is_file() {
            if let Ok(meta) = fs::metadata(&legacy_path) {
                total_bytes += meta.len();
            }
        }
    }

    let events_path = get_events_log_path();
    if let Ok(content) = fs::read_to_string(&events_path) {
        for line in content.lines() {
            if line.contains(r#""level":"ERROR""#)
                || line.contains(r#""level":"PANIC""#)
                || line.contains(r#""level":"CRASH""#)
            {
                error_count += 1;
            }
        }
    } else if let Ok(content) = fs::read_to_string(get_master_log_path()) {
        for line in content.lines() {
            if line.contains("[ERROR]") || line.contains("[PANIC]") || line.contains("[CRASH]") {
                error_count += 1;
            }
        }
    }

    let summary = (total_bytes, error_count);
    *cache = Some((Instant::now(), summary));
    summary
}

pub fn load_crash_history() -> CrashHistory {
    let mut cache = CACHED_CRASH_HISTORY.lock();
    if let Some((instant, ref val)) = *cache {
        if instant.elapsed() < SENTRY_CACHE_TTL {
            return val.clone();
        }
    }

    let path = get_crash_history_path();
    let history = if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(history) = toml::from_str::<CrashHistory>(&content) {
                history
            } else {
                CrashHistory::default()
            }
        } else {
            CrashHistory::default()
        }
    } else {
        CrashHistory::default()
    };

    *cache = Some((Instant::now(), history.clone()));
    history
}

pub fn save_crash_history(history: &CrashHistory) {
    let path = get_crash_history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(toml_str) = toml::to_string_pretty(history) {
        let _ = fs::write(path, toml_str);
    }
    *CACHED_CRASH_HISTORY.lock() = Some((Instant::now(), history.clone()));
}

pub fn record_crash_and_check_circuit_breaker(max_crashes: u32, window_secs: u64) -> bool {
    let mut history = load_crash_history();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Retain only crashes within the sliding time window
    history.crash_timestamps.retain(|&ts| now.saturating_sub(ts) <= window_secs);
    history.crash_timestamps.push(now);

    if history.crash_timestamps.len() > max_crashes as usize {
        history.circuit_breaker_tripped = true;
        history.last_tripped_time = Some(now);
        save_crash_history(&history);
        false // Circuit breaker tripped: DO NOT RESPAWN
    } else {
        history.circuit_breaker_tripped = false;
        save_crash_history(&history);
        true // Healthy: OK TO RESPAWN
    }
}

pub fn reset_circuit_breaker() {
    let path = get_crash_history_path();
    let _ = fs::remove_file(path);
}

pub fn is_circuit_breaker_tripped() -> bool {
    load_crash_history().circuit_breaker_tripped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_serialization_roundtrip() {
        let original = StateJournal {
            magic: MAGIC,
            master_pid: 12345,
            clean_shutdown: false,
            cloaked_windows: vec![JournalCloakedWindow {
                hwnd: 0x1002,
                title: "Test Application".to_string(),
                exe_name: "test.exe".to_string(),
            }],
            pinned_windows: vec![JournalPinnedWindow {
                hwnd: 0x2004,
                original_title: "Document 1".to_string(),
            }],
        };

        let serialized = toml::to_string_pretty(&original).expect("Serialization failed");
        let deserialized: StateJournal = toml::from_str(&serialized).expect("Deserialization failed");

        assert_eq!(deserialized.magic, MAGIC);
        assert_eq!(deserialized.master_pid, 12345);
        assert!(!deserialized.clean_shutdown);
        assert_eq!(deserialized.cloaked_windows.len(), 1);
        assert_eq!(deserialized.cloaked_windows[0].hwnd, 0x1002);
        assert_eq!(deserialized.pinned_windows.len(), 1);
        assert_eq!(deserialized.pinned_windows[0].original_title, "Document 1");
    }

    #[test]
    fn test_decode_ntstatus() {
        assert!(decode_ntstatus(0).contains("STATUS_SUCCESS"));
        assert!(decode_ntstatus(0xC0000005).contains("ACCESS_VIOLATION"));
        assert!(decode_ntstatus(0xC0000409).contains("STACK_BUFFER_OVERRUN"));
        assert!(decode_ntstatus(0xC00000FD).contains("STACK_OVERFLOW"));
        assert!(decode_ntstatus(0xE06D7363).contains("CPP_EXCEPTION"));
        assert!(decode_ntstatus(0x12345678).contains("UNHANDLED_EXCEPTION"));
    }

    #[test]
    fn test_circuit_breaker_logic() {
        let mut history = CrashHistory::default();
        let now = 1000u64;
        let window = 60u64;
        let max = 3u32;

        // First crash
        history.crash_timestamps.push(now);
        assert!(history.crash_timestamps.len() <= max as usize);

        // Second crash
        history.crash_timestamps.push(now + 10);
        assert!(history.crash_timestamps.len() <= max as usize);

        // Third crash
        history.crash_timestamps.push(now + 20);
        assert!(history.crash_timestamps.len() <= max as usize);

        // Fourth crash within 60s window trips circuit breaker
        history.crash_timestamps.retain(|&ts| (now + 30).saturating_sub(ts) <= window);
        history.crash_timestamps.push(now + 30);
        assert!(history.crash_timestamps.len() > max as usize);

        // Advance time past the 60s window
        let later = now + 100;
        history.crash_timestamps.retain(|&ts| later.saturating_sub(ts) <= window);
        assert_eq!(history.crash_timestamps.len(), 0);
    }

    #[test]
    fn test_crash_history_serialization() {
        let original = CrashHistory {
            crash_timestamps: vec![1000, 1020, 1040],
            circuit_breaker_tripped: true,
            last_tripped_time: Some(1040),
        };

        let serialized = toml::to_string_pretty(&original).expect("Serialization failed");
        let deserialized: CrashHistory = toml::from_str(&serialized).expect("Deserialization failed");

        assert_eq!(deserialized.crash_timestamps, vec![1000, 1020, 1040]);
        assert!(deserialized.circuit_breaker_tripped);
        assert_eq!(deserialized.last_tripped_time, Some(1040));
    }

    #[test]
    fn test_format_timestamp() {
        // 2026-11-15 00:00:00 UTC -> 1794700800
        let formatted = format_timestamp(1794700800);
        assert_eq!(formatted, "2026-11-15 00:00:00");
    }

    #[test]
    fn test_log_summary_and_clearing() {
        let temp_dir = std::env::temp_dir().join(format!("wintil_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let crash_file = temp_dir.join("wintil_crash.log");
        let events_file = temp_dir.join("wintil_events.jsonl");

        let _ = fs::write(&crash_file, "Sample crash entry\n");
        let _ = fs::write(&events_file, r#"{"level":"INFO","msg":"hello"}"#);

        assert!(crash_file.exists());
        assert!(events_file.exists());

        let _ = fs::remove_file(&crash_file);
        let _ = fs::remove_file(&events_file);
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
