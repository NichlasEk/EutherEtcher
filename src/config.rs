use std::{fs, path::Path};

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub flash: FlashConfig,
    #[serde(default)]
    pub safety: SafetyConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FlashConfig {
    pub image: Option<String>,
    pub device: Option<String>,
    #[serde(default)]
    pub verify_after_write: bool,
    pub chunk_size_mib: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_true")]
    pub refuse_internal_drives: bool,
    #[serde(default = "default_true")]
    pub refuse_mounted_devices: bool,
    #[serde(default = "default_max_device_size_gib")]
    pub max_device_size_gib_without_force: u64,
    #[serde(default = "default_true")]
    pub require_typed_confirmation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub show_progress: bool,
    #[serde(default)]
    pub verbose: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            refuse_internal_drives: true,
            refuse_mounted_devices: true,
            max_device_size_gib_without_force: default_max_device_size_gib(),
            require_typed_confirmation: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            verbose: false,
        }
    }
}

impl Config {
    pub fn from_path(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path)?;
        Ok(toml::from_str(&data)?)
    }
}

pub fn default_chunk_size_mib() -> u64 {
    4
}

fn default_true() -> bool {
    true
}

fn default_max_device_size_gib() -> u64 {
    256
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_toml_config() {
        let config: Config = toml::from_str(
            r#"
            [flash]
            image = "./archlinux.iso"
            device = "/dev/sdb"
            verify_after_write = true
            chunk_size_mib = 8

            [safety]
            refuse_internal_drives = true
            refuse_mounted_devices = false
            max_device_size_gib_without_force = 64
            require_typed_confirmation = true

            [ui]
            show_progress = false
            verbose = true
            "#,
        )
        .expect("valid config should parse");

        assert_eq!(config.flash.image.as_deref(), Some("./archlinux.iso"));
        assert_eq!(config.flash.device.as_deref(), Some("/dev/sdb"));
        assert!(config.flash.verify_after_write);
        assert_eq!(config.flash.chunk_size_mib, Some(8));
        assert!(config.safety.refuse_internal_drives);
        assert!(!config.safety.refuse_mounted_devices);
        assert_eq!(config.safety.max_device_size_gib_without_force, 64);
        assert!(config.safety.require_typed_confirmation);
        assert!(!config.ui.show_progress);
        assert!(config.ui.verbose);
    }

    #[test]
    fn defaults_missing_sections() {
        let config: Config = toml::from_str("").expect("empty config should use defaults");

        assert!(config.flash.image.is_none());
        assert!(config.safety.refuse_internal_drives);
        assert!(config.safety.refuse_mounted_devices);
        assert_eq!(config.safety.max_device_size_gib_without_force, 256);
        assert!(config.safety.require_typed_confirmation);
        assert!(config.ui.show_progress);
        assert!(!config.ui.verbose);
    }
}
