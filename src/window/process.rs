use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// Retrieves the Process ID (PID) for a given window HWND
pub fn get_window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0u32;
    unsafe {
        let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if tid == 0 || pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessDetails {
    pub exe_name: String,       // e.g. "chrome.exe"
    pub full_path: String,      // e.g. "C:\Users\ru\AppData\Local\imput\Helium\Application\chrome.exe"
    pub display_name: String,   // e.g. "Helium (chrome.exe)" or "Google Chrome (chrome.exe)"
    pub target_rule: String,    // e.g. "Helium" or "chrome.exe"
}

/// Retrieves the full executable path for a given PID
pub fn get_process_full_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;

        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        if res.is_ok() && size > 0 {
            Some(String::from_utf16_lossy(&buffer[..size as usize]))
        } else {
            None
        }
    }
}

/// Disambiguates process identity, differentiating forks sharing identical binary names (e.g. Helium vs Chrome)
pub fn get_process_details(pid: u32) -> Option<ProcessDetails> {
    let full_path = get_process_full_path(pid)?;
    let exe_name = std::path::Path::new(&full_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(&full_path)
        .to_string();

    let path_lower = full_path.to_ascii_lowercase();
    let (display_name, target_rule) = if exe_name.eq_ignore_ascii_case("chrome.exe") {
        if path_lower.contains(r"\helium\") || path_lower.contains("imput\\helium") {
            ("Helium (chrome.exe)".to_string(), "Helium".to_string())
        } else if path_lower.contains(r"\google\chrome\") {
            ("Google Chrome (chrome.exe)".to_string(), "chrome.exe".to_string())
        } else if path_lower.contains(r"\chromium\") {
            ("Chromium (chrome.exe)".to_string(), "Chromium".to_string())
        } else {
            (exe_name.clone(), exe_name.clone())
        }
    } else if exe_name.eq_ignore_ascii_case("msedge.exe") {
        ("Microsoft Edge".to_string(), "msedge.exe".to_string())
    } else if exe_name.eq_ignore_ascii_case("brave.exe") {
        ("Brave Browser".to_string(), "brave.exe".to_string())
    } else {
        (exe_name.clone(), exe_name.clone())
    };

    Some(ProcessDetails {
        exe_name,
        full_path,
        display_name,
        target_rule,
    })
}

/// Retrieves the process display name (disambiguated for forks like Helium) for a given PID
pub fn get_process_name(pid: u32) -> Option<String> {
    get_process_details(pid).map(|d| d.display_name)
}

/// Forcefully terminates the process associated with a PID
pub fn terminate_pid(pid: u32) -> Result<(), String> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|e| format!("Failed to open process (PID {}): {}", pid, e))?;

        let res = TerminateProcess(handle, 1);
        let _ = CloseHandle(handle);

        if res.is_ok() {
            Ok(())
        } else {
            Err(format!("TerminateProcess failed for PID {}", pid))
        }
    }
}

/// Forcefully terminates all running processes matching the target executable name
pub fn terminate_all_by_name(target_name: &str) -> Result<usize, String> {
    let target_lower = target_name.to_ascii_lowercase();
    let mut terminated_count = 0;

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| format!("CreateToolhelp32Snapshot failed: {}", e))?;

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                // Extract null-terminated exe file name
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let exe_name = String::from_utf16_lossy(&entry.szExeFile[..end]);

                if exe_name.to_ascii_lowercase() == target_lower {
                    let pid = entry.th32ProcessID;
                    if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                        if TerminateProcess(handle, 1).is_ok() {
                            terminated_count += 1;
                        }
                        let _ = CloseHandle(handle);
                    }
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }

    Ok(terminated_count)
}
