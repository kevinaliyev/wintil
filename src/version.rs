//! Universal versioning and application identity definitions.
//! Sourced directly from Cargo.toml at compile-time via standard env! macros.

pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
pub const APP_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

/// Returns the clean semantic version string (e.g. "0.2.0")
pub fn version() -> &'static str {
    APP_VERSION
}

/// Returns the formatted banner string for UI footers and CLI headers
pub fn banner() -> String {
    format!("Wintil v{} • Standalone Native Windows PE", APP_VERSION)
}

/// Returns the full application title for window titles or logs
pub fn full_identity() -> String {
    format!("{} v{} - {}", APP_NAME, APP_VERSION, APP_DESCRIPTION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_not_empty() {
        assert!(!version().is_empty());
        assert!(!APP_NAME.is_empty());
        assert!(banner().contains(version()));
    }
}
