pub mod keys;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pin::stroke::StrokeConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub hotkeys: HotkeysConfig,
    #[serde(default)]
    pub hider: HiderConfig,
    #[serde(default)]
    pub tray: TrayConfig,
    #[serde(default)]
    pub pin_hud: PinHudConfig,
    #[serde(default)]
    pub pin_stroke: StrokeConfig,
    #[serde(default)]
    pub sentry: SentryConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            hotkeys: HotkeysConfig::default(),
            hider: HiderConfig::default(),
            tray: TrayConfig::default(),
            pin_hud: PinHudConfig::default(),
            pin_stroke: StrokeConfig::default(),
            sentry: SentryConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryConfig {
    #[serde(default = "default_true")]
    pub logging_enabled: bool,
    #[serde(default = "default_true")]
    pub respawn_on_crash: bool,
    #[serde(default = "default_max_crashes")]
    pub max_crashes: u32,
    #[serde(default = "default_crash_window_secs")]
    pub crash_window_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_max_crashes() -> u32 {
    3
}

fn default_crash_window_secs() -> u64 {
    60
}

impl Default for SentryConfig {
    fn default() -> Self {
        Self {
            logging_enabled: true,
            respawn_on_crash: true,
            max_crashes: 3,
            crash_window_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinHudConfig {
    #[serde(default = "default_pinned_text")]
    pub text_pinned: String,
    #[serde(default = "default_unpinned_text")]
    pub text_unpinned: String,
    #[serde(default = "default_hud_bg")]
    pub bg_color: String,
    #[serde(default = "default_hud_text_color")]
    pub text_color: String,
    #[serde(default = "default_hud_stroke_color")]
    pub stroke_color: String,
    #[serde(default = "default_hud_opacity")]
    pub opacity: f32,
}

impl Default for PinHudConfig {
    fn default() -> Self {
        Self {
            text_pinned: "Pinned".to_string(),
            text_unpinned: "Unpinned".to_string(),
            bg_color: "#383E48".to_string(),
            text_color: "#FFFFFF".to_string(),
            stroke_color: "#000000".to_string(),
            opacity: 0.75,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_true")]
    pub single_instance: bool,
    #[serde(default = "default_true")]
    pub show_pin_hud: bool,
    #[serde(default = "default_true")]
    pub pin_title_tag: bool,
    #[serde(default)]
    pub show_tooltips: bool,
    #[serde(default = "default_notification_duration")]
    pub notification_duration_ms: u64,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            single_instance: true,
            show_pin_hud: true,
            pin_title_tag: true,
            show_tooltips: false,
            notification_duration_ms: 800,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeysConfig {
    #[serde(default = "default_maximize")]
    pub maximize_toggle: String,
    #[serde(default = "default_fake_fullscreen")]
    pub fake_fullscreen: String,
    #[serde(default = "default_minimize")]
    pub minimize_active: String,
    #[serde(default = "default_close")]
    pub close_active: String,
    #[serde(default = "default_kill")]
    pub kill_active: String,
    #[serde(default = "default_kill_all")]
    pub kill_all_by_exe: String,
    #[serde(default = "default_hide_tray")]
    pub hide_to_tray: String,
    #[serde(default = "default_toggle_pin")]
    pub toggle_pin: String,
    #[serde(default = "default_restore_last")]
    pub restore_last: String,
    #[serde(default = "default_restore_all")]
    pub restore_all: String,
    #[serde(default = "default_unstick_keys")]
    pub unstick_keys: String,
}

fn default_fake_fullscreen() -> String {
    "Alt+Shift+F".to_string()
}

fn default_unstick_keys() -> String {
    "Alt+Shift+Backspace".to_string()
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            maximize_toggle: "Alt+F".to_string(),
            fake_fullscreen: "Alt+Shift+F".to_string(),
            minimize_active: "Alt+H".to_string(),
            close_active: "Alt+Q".to_string(),
            kill_active: "Alt+Shift+Q".to_string(),
            kill_all_by_exe: "Alt+Ctrl+Shift+Q".to_string(),
            hide_to_tray: "Alt+Shift+H".to_string(),
            toggle_pin: "Alt+P".to_string(),
            restore_last: "Alt+Shift+U".to_string(),
            restore_all: "Alt+Shift+R".to_string(),
            unstick_keys: "Alt+Shift+Backspace".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiderConfig {
    #[serde(default = "default_service_interval", alias = "daemon_interval_ms")]
    pub service_interval_ms: u64,
    #[serde(default = "default_auto_hide_exes")]
    pub auto_hide_executables: Vec<String>,
    #[serde(default = "default_true")]
    pub restore_all_on_exit: bool,
}

impl Default for HiderConfig {
    fn default() -> Self {
        Self {
            service_interval_ms: 750,
            auto_hide_executables: Vec::new(),
            restore_all_on_exit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrayConfig {
    #[serde(default = "default_tray_tooltip")]
    pub tooltip: String,
    #[serde(default = "default_icon_path")]
    pub icon_path: String,
}

impl Default for TrayConfig {
    fn default() -> Self {
        Self {
            tooltip: format!("Wintil v{}", crate::version::APP_VERSION),
            icon_path: "assets/icon.ico".to_string(),
        }
    }
}

fn default_notification_duration() -> u64 {
    800
}

fn default_maximize() -> String {
    "Alt+F".to_string()
}

fn default_minimize() -> String {
    "Alt+H".to_string()
}

fn default_close() -> String {
    "Alt+Q".to_string()
}

fn default_kill() -> String {
    "Alt+Shift+Q".to_string()
}

fn default_kill_all() -> String {
    "Alt+Ctrl+Shift+Q".to_string()
}

fn default_hide_tray() -> String {
    "Alt+Shift+H".to_string()
}

fn default_toggle_pin() -> String {
    "Alt+P".to_string()
}

fn default_restore_last() -> String {
    "Alt+Shift+U".to_string()
}

fn default_restore_all() -> String {
    "Alt+Shift+R".to_string()
}

fn default_service_interval() -> u64 {
    750
}

fn default_auto_hide_exes() -> Vec<String> {
    Vec::new()
}

fn default_tray_tooltip() -> String {
    format!("Wintil v{}", crate::version::APP_VERSION)
}

fn default_icon_path() -> String {
    "assets/icon.ico".to_string()
}

/// Returns the canonical configuration directory: $HOME/.config/wintil
pub fn get_config_dir() -> PathBuf {
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join(".config").join("wintil")
}

/// Returns the canonical log master directory: $HOME/.config/wintil/logs
pub fn get_logs_dir() -> PathBuf {
    let dir = get_config_dir().join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Resolves the configuration file path, checking $HOME/.config/wintil/wintil.toml first,
/// creating the directory if it does not yet exist.
pub fn get_config_path() -> PathBuf {
    let dir = get_config_dir();
    let primary_file = dir.join("wintil.toml");
    if primary_file.exists() {
        return primary_file;
    }

    // Alternative location directly under .config: $HOME/.config/wintil.toml
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        let alt_file = PathBuf::from(home).join(".config").join("wintil.toml");
        if alt_file.exists() {
            return alt_file;
        }
    }

    // Check current working directory
    let cwd_path = PathBuf::from("wintil.toml");
    if cwd_path.exists() {
        return cwd_path;
    }

    // Check adjacent to exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let exe_dir_path = parent.join("wintil.toml");
            if exe_dir_path.exists() {
                return exe_dir_path;
            }
        }
    }

    // Primary target: ensure $HOME/.config/wintil directory exists and return primary file
    let _ = fs::create_dir_all(&dir);
    primary_file
}

impl AppConfig {
    /// Loads configuration from the specified path or returns default
    pub fn load_from_path(path: &Path) -> Self {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                if let Ok(config) = toml::from_str::<AppConfig>(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }

    /// Saves configuration to the specified path
    pub fn save_to_path(&self, path: &Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        fs::write(path, content)
    }

    /// Fetches configuration from $HOME/.config, creating the directory and default file if it doesn't exist
    pub fn load() -> (Self, PathBuf) {
        let config_path = get_config_path();
        if config_path.exists() {
            (Self::load_from_path(&config_path), config_path)
        } else {
            let default_cfg = Self::default();
            let _ = default_cfg.save_to_path(&config_path);
            (default_cfg, config_path)
        }
    }
}

fn default_pinned_text() -> String {
    "Pinned".to_string()
}

fn default_unpinned_text() -> String {
    "Unpinned".to_string()
}

fn default_hud_bg() -> String {
    "#383E48".to_string()
}

fn default_hud_text_color() -> String {
    "#FFFFFF".to_string()
}

fn default_hud_stroke_color() -> String {
    "#000000".to_string()
}

fn default_hud_opacity() -> f32 {
    0.75
}

/// Parses a color string (e.g. "#383E48", "#FFF", or preset names) into (r, g, b) float tuples
pub fn parse_color_rgb(s: &str, default: (f32, f32, f32)) -> (f32, f32, f32) {
    let s = s.trim();
    if s.starts_with('#') {
        let hex = &s[1..];
        if hex.len() == 6 {
            if let Ok(num) = u32::from_str_radix(hex, 16) {
                let r = ((num >> 16) & 0xFF) as f32;
                let g = ((num >> 8) & 0xFF) as f32;
                let b = (num & 0xFF) as f32;
                return (r, g, b);
            }
        } else if hex.len() == 3 {
            if let Ok(num) = u32::from_str_radix(hex, 16) {
                let r = (((num >> 8) & 0xF) * 17) as f32;
                let g = (((num >> 4) & 0xF) * 17) as f32;
                let b = ((num & 0xF) * 17) as f32;
                return (r, g, b);
            }
        }
    }

    match s.to_ascii_lowercase().as_str() {
        "slate" => (58.0, 64.0, 74.0),
        "charcoal" => (28.0, 30.0, 36.0),
        "black" => (0.0, 0.0, 0.0),
        "white" => (255.0, 255.0, 255.0),
        "burgundy" => (128.0, 20.0, 42.0),
        "crimson" => (220.0, 38.0, 38.0),
        "cyan" => (6.0, 182.0, 212.0),
        "emerald" => (16.0, 185.0, 129.0),
        "gold" | "amber" => (245.0, 158.0, 11.0),
        "navy" => (24.0, 32.0, 56.0),
        "blue" => (37.0, 99.0, 235.0),
        "violet" | "purple" => (139.0, 92.0, 246.0),
        _ => default,
    }
}
