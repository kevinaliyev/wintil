use std::fs;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIIF_INFO, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, LoadIconW, LoadImageW, HICON, IDI_APPLICATION,
    IMAGE_ICON, LR_DEFAULTCOLOR, LR_DEFAULTSIZE, LR_LOADFROMFILE, WM_APP,
};

pub const WM_APP_TRAY: u32 = WM_APP + 10;
const TRAY_ICON_ID: u32 = 1;

// Embed the icon asset directly into the binary so the tray icon is never missing
const EMBEDDED_ICON_ICO: &[u8] = include_bytes!("../../assets/icon.ico");

pub struct TrayManager {
    hwnd: HWND,
    hicon: HICON,
    is_custom_icon: bool,
}

impl TrayManager {
    pub fn new(hwnd: HWND, icon_path: &str) -> Self {
        let (hicon, is_custom_icon) = Self::load_icon(icon_path);
        Self {
            hwnd,
            hicon,
            is_custom_icon,
        }
    }

    fn load_icon(path_str: &str) -> (HICON, bool) {
        // 1. Try loading from specified user path on disk
        if !path_str.is_empty() {
            let p = Path::new(path_str);
            if p.exists() {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                if ext == "ico" {
                    let wide: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
                    unsafe {
                        if let Ok(handle) = LoadImageW(
                            None,
                            PCWSTR(wide.as_ptr()),
                            IMAGE_ICON,
                            0,
                            0,
                            LR_LOADFROMFILE | LR_DEFAULTSIZE,
                        ) {
                            let hicon = HICON(handle.0);
                            if !hicon.0.is_null() {
                                return (hicon, true);
                            }
                        }
                    }
                } else if ext == "png" {
                    if let Ok(bytes) = fs::read(p) {
                        if let Some(hicon) = Self::create_icon_from_png_bytes(&bytes) {
                            return (hicon, true);
                        }
                    }
                }
            }
        }

        // 2. Check if assets/icon.ico exists adjacent to executable or in assets/
        for candidate in &["assets/icon.ico", "icon.ico", "wintil/assets/icon.ico"] {
            let p = Path::new(candidate);
            if p.exists() {
                let wide: Vec<u16> = candidate.encode_utf16().chain(std::iter::once(0)).collect();
                unsafe {
                    if let Ok(handle) = LoadImageW(
                        None,
                        PCWSTR(wide.as_ptr()),
                        IMAGE_ICON,
                        0,
                        0,
                        LR_LOADFROMFILE | LR_DEFAULTSIZE,
                    ) {
                        let hicon = HICON(handle.0);
                        if !hicon.0.is_null() {
                            return (hicon, true);
                        }
                    }
                }
            }
        }

        // 3. Fallback to embedded ICO icon resource
        if let Some(hicon) = Self::create_icon_from_ico_bytes(EMBEDDED_ICON_ICO) {
            return (hicon, true);
        }

        // 4. Fallback to standard system application icon
        unsafe {
            let hicon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
            (hicon, false)
        }
    }

    fn create_icon_from_ico_bytes(ico_bytes: &[u8]) -> Option<HICON> {
        if ico_bytes.len() < 22 {
            return None;
        }
        let count = u16::from_le_bytes([ico_bytes[4], ico_bytes[5]]) as usize;
        if count == 0 || ico_bytes.len() < 6 + count * 16 {
            return None;
        }
        // Pick the largest entry (e.g. 32x32 or 48x48 or 256x256)
        let mut best_slice: Option<&[u8]> = None;
        let mut best_score: u32 = 0;
        for i in 0..count {
            let entry = &ico_bytes[6 + i * 16..6 + (i + 1) * 16];
            let w = if entry[0] == 0 { 256 } else { entry[0] as u32 };
            let h = if entry[1] == 0 { 256 } else { entry[1] as u32 };
            let size = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
            let offset = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
            if offset + size <= ico_bytes.len() && size > 0 {
                let score = w * h;
                if score >= best_score {
                    best_score = score;
                    best_slice = Some(&ico_bytes[offset..offset + size]);
                }
            }
        }
        best_slice.and_then(Self::create_icon_from_png_bytes)
    }

    fn create_icon_from_png_bytes(png_bytes: &[u8]) -> Option<HICON> {
        unsafe {
            let hicon = CreateIconFromResourceEx(
                png_bytes,
                true,
                0x00030000,
                0,
                0,
                LR_DEFAULTCOLOR,
            ).ok()?;

            if !hicon.0.is_null() {
                Some(hicon)
            } else {
                None
            }
        }
    }

    /// Adds the icon to the Windows notification tray.
    /// Returns true if successfully added, or false if the Shell is not yet ready.
    pub fn add(&self, tooltip: &str) -> bool {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NOTIFY_ICON_DATA_FLAGS(0x01 | 0x02 | 0x04), // NIF_MESSAGE | NIF_ICON | NIF_TIP
            uCallbackMessage: WM_APP_TRAY,
            hIcon: self.hicon,
            ..Default::default()
        };

        let tip_chars: Vec<u16> = tooltip.encode_utf16().collect();
        let max_len = nid.szTip.len() - 1;
        let copy_len = tip_chars.len().min(max_len);
        nid.szTip[..copy_len].copy_from_slice(&tip_chars[..copy_len]);

        unsafe {
            if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                return true;
            }
            // If NIM_ADD failed, an orphaned registration from an ungracefully terminated
            // instance might still be lingering in the notification area.
            // Attempt to clear any stale entry and retry NIM_ADD once.
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            Shell_NotifyIconW(NIM_ADD, &nid).as_bool()
        }
    }

    /// Shows a system notification (balloon / toast)
    pub fn show_toast(&self, title: &str, message: &str) {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ICON_ID,
            uFlags: NOTIFY_ICON_DATA_FLAGS(0x10), // NIF_INFO
            dwInfoFlags: NIIF_INFO,
            ..Default::default()
        };

        let title_chars: Vec<u16> = title.encode_utf16().collect();
        let title_max = nid.szInfoTitle.len() - 1;
        let title_len = title_chars.len().min(title_max);
        nid.szInfoTitle[..title_len].copy_from_slice(&title_chars[..title_len]);

        let msg_chars: Vec<u16> = message.encode_utf16().collect();
        let msg_max = nid.szInfo.len() - 1;
        let msg_len = msg_chars.len().min(msg_max);
        nid.szInfo[..msg_len].copy_from_slice(&msg_chars[..msg_len]);

        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Removes the icon from the Windows notification tray
    pub fn remove(&self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ICON_ID,
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        self.remove();
        if self.is_custom_icon && !self.hicon.0.is_null() {
            unsafe {
                let _ = DestroyIcon(self.hicon);
            }
        }
    }
}
