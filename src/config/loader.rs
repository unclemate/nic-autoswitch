//! Configuration loader
//!
//! This module provides configuration file loading and parsing capabilities.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

use super::schema::Config;
use crate::{NicAutoSwitchError, Result};

/// Configuration loader with hot-reload support
#[derive(Debug)]
pub struct ConfigLoader {
    path: PathBuf,
    current: Arc<Config>,
    sender: Option<watch::Sender<Arc<Config>>>,
}

impl ConfigLoader {
    /// Create a new configuration loader
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let config = Self::load_from_path(&path)?;
        let current = Arc::new(config);
        let (sender, _) = watch::channel(current.clone());

        Ok(Self {
            path,
            current,
            sender: Some(sender),
        })
    }

    /// Load configuration from file
    pub fn load(&self) -> Result<Config> {
        Self::load_from_path(&self.path)
    }

    /// Get current configuration
    pub fn current(&self) -> Arc<Config> {
        self.current.clone()
    }

    /// Get a receiver for configuration updates
    pub fn subscribe(&self) -> watch::Receiver<Arc<Config>> {
        self.sender
            .as_ref()
            .expect("ConfigLoader always initializes with a sender")
            .subscribe()
    }

    /// Override dry_run flag from CLI argument
    pub fn override_dry_run(&mut self, dry_run: bool) {
        if dry_run && !self.current.global.dry_run {
            let mut config = (*self.current).clone();
            config.global.dry_run = true;
            self.current = Arc::new(config);
            if let Some(sender) = &self.sender {
                let _ = sender.send(self.current.clone());
            }
        }
    }

    /// Reload configuration from file
    pub fn reload(&mut self) -> Result<Arc<Config>> {
        let config = Self::load_from_path(&self.path)?;
        config.validate()?;
        self.current = Arc::new(config.clone());

        if let Some(sender) = &self.sender
            && sender.send(self.current.clone()).is_err()
        {
            warn!("No active subscribers for config updates");
        }

        info!("Configuration reloaded successfully");
        Ok(self.current.clone())
    }

    /// Load configuration from a specific path
    fn load_from_path(path: &Path) -> Result<Config> {
        if !path.exists() {
            return Err(NicAutoSwitchError::Config(format!(
                "Configuration file not found: {}",
                path.display()
            )));
        }

        let content = fs::read_to_string(path).map_err(|e| {
            NicAutoSwitchError::Config(format!("Failed to read {}: {}", path.display(), e))
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| {
            NicAutoSwitchError::Config(format!("Failed to parse {}: {}", path.display(), e))
        })?;

        config.validate()?;
        Ok(config)
    }

    /// Get the configuration file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Load configuration from a file path (convenience function)
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    ConfigLoader::load_from_path(path.as_ref())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    fn get_valid_config_content() -> &'static str {
        r#"
[global]
monitor_interval = 5
log_level = "info"
dry_run = false
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10
"#
    }

    #[test]
    fn test_load_config_with_valid_file_succeeds() {
        let file = create_temp_config(get_valid_config_content());
        let result = ConfigLoader::new(file.path());

        assert!(result.is_ok());
        let loader = result.unwrap();
        assert_eq!(loader.current().global.monitor_interval, 5);
    }

    #[test]
    fn test_load_config_with_missing_file_returns_error() {
        let result = ConfigLoader::new("/nonexistent/config.toml");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_load_config_with_invalid_toml_returns_error() {
        let file = create_temp_config("invalid [[[ toml");
        let result = ConfigLoader::new(file.path());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Failed to parse"));
    }

    #[test]
    fn test_load_config_with_empty_interfaces_returns_error() {
        let content = r#"
[global]
monitor_interval = 5
"#;
        let file = create_temp_config(content);
        let result = ConfigLoader::new(file.path());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("At least one interface"));
    }

    #[test]
    fn test_load_config_with_invalid_table_id_returns_error() {
        let content = r#"
[global]
monitor_interval = 5
table_id_start = 50

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
        let file = create_temp_config(content);
        let result = ConfigLoader::new(file.path());

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("table_id_start"));
    }

    #[test]
    fn test_reload_updates_configuration() {
        let file = create_temp_config(get_valid_config_content());
        let mut loader = ConfigLoader::new(file.path()).unwrap();

        assert_eq!(loader.current().global.monitor_interval, 5);

        // Update the file
        let updated_content = r#"
[global]
monitor_interval = 10
log_level = "debug"

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
        std::fs::write(file.path(), updated_content).unwrap();

        // Reload
        let result = loader.reload();
        assert!(result.is_ok());
        assert_eq!(loader.current().global.monitor_interval, 10);
        assert_eq!(loader.current().global.log_level, "debug");
    }

    #[test]
    fn test_subscribe_receives_updates() {
        let file = create_temp_config(get_valid_config_content());
        let mut loader = ConfigLoader::new(file.path()).unwrap();

        let receiver = loader.subscribe();

        // Initial value should be available
        assert_eq!(receiver.borrow().global.monitor_interval, 5);

        // Update and reload
        let updated_content = r#"
[global]
monitor_interval = 15
log_level = "info"

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
        std::fs::write(file.path(), updated_content).unwrap();
        loader.reload().unwrap();

        // Receiver should have the updated value
        assert_eq!(receiver.borrow().global.monitor_interval, 15);
    }

    #[test]
    fn test_convenience_function_loads_config() {
        let file = create_temp_config(get_valid_config_content());
        let result = load_config(file.path());

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.global.monitor_interval, 5);
    }

    #[test]
    fn test_loader_path_returns_config_path() {
        let file = create_temp_config(get_valid_config_content());
        let loader = ConfigLoader::new(file.path()).unwrap();

        assert_eq!(loader.path(), file.path());
    }

    #[test]
    fn test_load_config_with_wifi_profile_succeeds() {
        let content = r#"
[global]
monitor_interval = 5

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }

[wifi_profiles."CorpWiFi"]
interface = "wlan0"

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-cidr"
match_on = { cidr = "10.0.0.0/8" }
route_via = { interface = "eth0" }
priority = 100
"#;
        let file = create_temp_config(content);
        let result = ConfigLoader::new(file.path());

        assert!(result.is_ok());
        let config = result.unwrap().current();
        assert!(config.wifi_profiles.contains_key("CorpWiFi"));
        assert_eq!(config.wifi_profiles["CorpWiFi"].rules.len(), 1);
    }

    #[test]
    fn test_load_config_with_default_rules_succeeds() {
        let content = r#"
[global]
monitor_interval = 5

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "default-wlan"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "eth0" }
priority = 10000
"#;
        let file = create_temp_config(content);
        let result = ConfigLoader::new(file.path());

        assert!(result.is_ok());
        let config = result.unwrap().current();
        assert_eq!(config.routing.default_rules.len(), 1);
        assert_eq!(config.routing.default_rules[0].name, "default-wlan");
    }
}
