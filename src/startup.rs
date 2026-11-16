use std::os::windows::process::CommandExt;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use windows::core::w;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

const CREATE_NO_WINDOW: u32 = 0x08000000;
static CACHED_TASK_STATUS: Mutex<Option<(Instant, StartupTaskStatus)>> = Mutex::new(None);
const TASK_STATUS_CACHE_TTL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartupTaskStatus {
    pub is_installed: bool,
    pub is_enabled: bool,
    pub is_highest_privilege: bool,
}

impl Default for StartupTaskStatus {
    fn default() -> Self {
        Self {
            is_installed: false,
            is_enabled: false,
            is_highest_privilege: false,
        }
    }
}

/// Invalidate any cached status so the next query reads fresh from Task Scheduler.
pub fn invalidate_task_status_cache() {
    *CACHED_TASK_STATUS.lock() = None;
}

/// Queries Windows Task Scheduler for the "Wintil" task status via XML definition.
/// Caches the result in memory for 5 seconds to eliminate subprocess lag on hover/paint.
pub fn query_task_status() -> StartupTaskStatus {
    let mut cache = CACHED_TASK_STATUS.lock();
    if let Some((instant, status)) = *cache {
        if instant.elapsed() < TASK_STATUS_CACHE_TTL {
            return status;
        }
    }

    let mut cmd = std::process::Command::new("schtasks.exe");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args(["/query", "/tn", "Wintil", "/xml"]);

    let status = match cmd.output() {
        Ok(out) if out.status.success() => {
            let xml = String::from_utf8_lossy(&out.stdout);
            parse_task_xml(&xml)
        }
        _ => StartupTaskStatus::default(),
    };

    *cache = Some((Instant::now(), status));
    status
}

pub fn parse_task_xml(xml: &str) -> StartupTaskStatus {
    let is_installed = xml.contains("<Task");
    let is_enabled = is_installed && !xml.contains("<Enabled>false</Enabled>");
    let is_highest_privilege = xml.contains("<RunLevel>HighestAvailable</RunLevel>");
    StartupTaskStatus {
        is_installed,
        is_enabled,
        is_highest_privilege,
    }
}

/// Toggles the "Wintil" startup task on or off in Task Scheduler.
pub fn toggle_task() {
    let status = query_task_status();
    if status.is_installed {
        if status.is_enabled {
            let mut cmd = std::process::Command::new("schtasks.exe");
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.args(["/change", "/tn", "Wintil", "/disable"]);
            let _ = cmd.output();
        } else {
            let mut cmd = std::process::Command::new("schtasks.exe");
            cmd.creation_flags(CREATE_NO_WINDOW);
            cmd.args(["/change", "/tn", "Wintil", "/enable"]);
            let _ = cmd.output();
        }
    } else {
        create_or_update_task(true);
    }
    invalidate_task_status_cache();
}

/// Toggles between HighestAvailable (elevated) and LeastPrivilege (standard) run level.
pub fn toggle_elevation() {
    let status = query_task_status();
    create_or_update_task(!status.is_highest_privilege);
}

/// Creates or updates the "Wintil" scheduled task with the given privilege level.
/// Uses XML registration to guarantee that laptop battery power transitions and idle resumes
/// never cause Windows Task Scheduler to prematurely terminate the background process.
pub fn create_or_update_task(highest_privilege: bool) {
    let exe_path = match std::env::current_exe() {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => return,
    };

    let rl = if highest_privilege { "HighestAvailable" } else { "LeastPrivilege" };

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Wintil</Author>
    <URI>\Wintil</URI>
  </RegistrationInfo>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>{rl}</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
  </Settings>
  <Triggers>
    <LogonTrigger />
  </Triggers>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe_path}"</Command>
    </Exec>
  </Actions>
</Task>"#
    );

    let temp_xml_path = std::env::temp_dir().join("wintil_task_def.xml");
    let wide_xml: Vec<u16> = std::iter::once(0xFEFF).chain(xml.encode_utf16()).collect();
    let bytes: Vec<u8> = wide_xml.into_iter().flat_map(|u| u.to_le_bytes()).collect();
    if std::fs::write(&temp_xml_path, &bytes).is_err() {
        return;
    }

    // Try direct command execution first (succeeds if already elevated or for LeastPrivilege)
    let mut cmd = std::process::Command::new("schtasks.exe");
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd.args(["/create", "/tn", "Wintil", "/xml", &temp_xml_path.to_string_lossy(), "/f"]);

    if let Ok(out) = cmd.output() {
        if out.status.success() {
            let _ = std::fs::remove_file(&temp_xml_path);
            invalidate_task_status_cache();
            return;
        }
    }

    // Direct registration failed (typically Access Denied when attempting HighestAvailable as standard user).
    // Prompt for administrative UAC elevation via Win32 ShellExecuteW runas.
    if highest_privilege {
        let params = format!(
            "/create /tn \"Wintil\" /xml \"{}\" /f",
            temp_xml_path.display()
        );
        let params_wide: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let _ = ShellExecuteW(
                None,
                w!("runas"),
                w!("schtasks.exe"),
                windows::core::PCWSTR(params_wide.as_ptr()),
                None,
                SW_HIDE,
            );
        }
    }
    invalidate_task_status_cache();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_task_xml_installed_highest() {
        let sample = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.3" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Principals>
    <Principal id="Author">
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
</Task>"#;
        let st = parse_task_xml(sample);
        assert!(st.is_installed);
        assert!(st.is_enabled);
        assert!(st.is_highest_privilege);
    }

    #[test]
    fn test_parse_task_xml_disabled_limited() {
        let sample = r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.3" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <Settings>
    <Enabled>false</Enabled>
  </Settings>
  <Principals>
    <Principal id="Author">
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
</Task>"#;
        let st = parse_task_xml(sample);
        assert!(st.is_installed);
        assert!(!st.is_enabled);
        assert!(!st.is_highest_privilege);
    }

    #[test]
    fn test_parse_task_xml_missing() {
        let sample = "ERROR: The system cannot find the file specified.";
        let st = parse_task_xml(sample);
        assert!(!st.is_installed);
        assert!(!st.is_enabled);
        assert!(!st.is_highest_privilege);
    }
}

