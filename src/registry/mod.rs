//! Registry Engine (Roadmap / Future Extension)
//!
//! Designed for managing declarative Windows registry adjustments that remain
//! persistent across cumulative Windows updates.
//!
//! Architectural Concept:
//! 1. RegistryManifest: Defines targeted keys, expected types (DWORD, SZ, QWORD), and desired data.
//! 2. RollbackStore: Automatically creates timestamped .reg backups prior to writing modifications.
//! 3. AutoHealer: Listens for OS modifications via `RegNotifyChangeKeyValue` or startup checks
//!    to re-enforce user preferences after Patch Tuesday updates.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryHive {
    CurrentUser,
    LocalMachine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegistryValueType {
    Dword,
    String,
    Qword,
    Binary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryTweak {
    pub name: String,
    pub description: String,
    pub hive: RegistryHive,
    pub subkey: String,
    pub value_name: String,
    pub value_type: RegistryValueType,
    pub target_data: String,
    pub backup_data: Option<String>,
}

pub struct RegistryManager {
    // Future implementation: RegOpenKeyExW, RegSetValueExW, RegNotifyChangeKeyValue
}

impl RegistryManager {
    pub fn new() -> Self {
        Self {}
    }
}
