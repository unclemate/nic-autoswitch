//! Configuration file watcher
//!
//! This module provides file system watching for configuration hot-reload.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{info, warn};

use super::Config;
use crate::config::load_config;

/// Configuration reload event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigEvent {
    /// Configuration file was modified
    Modified,
    /// Configuration file was deleted
    Deleted,
}

/// Configuration file watcher
pub struct ConfigWatcher {
    /// Path to configuration file
    config_path: PathBuf,
    /// Event sender
    event_tx: broadcast::Sender<ConfigEvent>,
    /// Whether watching is enabled
    enabled: bool,
    /// Shutdown flag for the spawned task
    shutdown: Arc<AtomicBool>,
}

impl ConfigWatcher {
    /// Create a new configuration watcher
    pub fn new(config_path: PathBuf) -> Self {
        let (event_tx, _) = broadcast::channel(16);

        Self {
            config_path,
            event_tx,
            enabled: false,
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Subscribe to configuration events
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigEvent> {
        self.event_tx.subscribe()
    }

    /// Start watching for configuration changes
    pub async fn start(&mut self) -> crate::Result<()> {
        info!("Starting configuration watcher for: {:?}", self.config_path);

        // For now, use a simple polling-based watcher
        // In production, you'd use notify crate for inotify-based watching
        self.enabled = true;

        // Spawn the watcher task
        let config_path = self.config_path.clone();
        let event_tx = self.event_tx.clone();
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            let mut last_modified = Self::get_modified_time(&config_path);

            loop {
                if shutdown.load(Ordering::Relaxed) {
                    info!("Configuration watcher task exiting");
                    break;
                }

                tokio::time::sleep(Duration::from_secs(5)).await;

                let current_modified = Self::get_modified_time(&config_path);

                match (last_modified, current_modified) {
                    (Some(last), Some(current)) if current > last => {
                        info!("Configuration file modified, sending reload event");
                        if let Err(e) = event_tx.send(ConfigEvent::Modified) {
                            warn!("Failed to send config event: {}", e);
                        }
                    }
                    (Some(_), None) => {
                        warn!("Configuration file deleted!");
                        if let Err(e) = event_tx.send(ConfigEvent::Deleted) {
                            warn!("Failed to send config event: {}", e);
                        }
                    }
                    _ => {}
                }

                last_modified = current_modified;
            }
        });

        info!("Configuration watcher started");
        Ok(())
    }

    /// Stop watching for configuration changes
    pub fn stop(&mut self) {
        self.enabled = false;
        self.shutdown.store(true, Ordering::Relaxed);
        info!("Configuration watcher stopped");
    }

    /// Check if watching is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get file modification time
    fn get_modified_time(path: &PathBuf) -> Option<std::time::SystemTime> {
        std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
    }

    /// Reload configuration from disk
    pub async fn reload(&self) -> crate::Result<Config> {
        info!("Reloading configuration from {:?}", self.config_path);
        load_config(&self.config_path)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_config() -> (NamedTempFile, PathBuf) {
        let mut file = NamedTempFile::new().unwrap();
        let content = r#"
[global]
log_level = "info"

monitor_interval = 5

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10
"#;
        file.write_all(content.as_bytes()).unwrap();
        let path = file.path().to_path_buf();
        (file, path)
    }

    #[test]
    fn test_config_watcher_new() {
        let (_file, path) = create_temp_config();
        let watcher = ConfigWatcher::new(path);
        assert!(!watcher.is_enabled());
    }

    #[test]
    fn test_config_watcher_subscribe() {
        let (_file, path) = create_temp_config();
        let watcher = ConfigWatcher::new(path);
        let _receiver = watcher.subscribe();
    }

    #[tokio::test]
    async fn test_config_watcher_start() {
        let (_file, path) = create_temp_config();
        let mut watcher = ConfigWatcher::new(path);
        let result = watcher.start().await;
        assert!(result.is_ok());
        assert!(watcher.is_enabled());
    }

    #[test]
    fn test_get_modified_time_existing_file() {
        let (_file, path) = create_temp_config();
        let modified = ConfigWatcher::get_modified_time(&path);
        assert!(modified.is_some());
    }

    #[test]
    fn test_get_modified_time_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/file.toml");
        let modified = ConfigWatcher::get_modified_time(&path);
        assert!(modified.is_none());
    }

    #[test]
    fn test_config_event_equality() {
        assert_eq!(ConfigEvent::Modified, ConfigEvent::Modified);
        assert_eq!(ConfigEvent::Deleted, ConfigEvent::Deleted);
        assert_ne!(ConfigEvent::Modified, ConfigEvent::Deleted);
    }

    #[test]
    fn test_config_event_debug() {
        assert!(format!("{:?}", ConfigEvent::Modified).contains("Modified"));
        assert!(format!("{:?}", ConfigEvent::Deleted).contains("Deleted"));
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_config_watcher_stop_disables() {
        let (_file, path) = create_temp_config();
        let mut watcher = ConfigWatcher::new(path);
        assert!(!watcher.is_enabled());

        // Start then stop
        // Note: start() spawns a task, we can't easily await it here
        watcher.enabled = true;
        assert!(watcher.is_enabled());

        watcher.stop();
        assert!(!watcher.is_enabled());
    }

    #[tokio::test]
    async fn test_config_watcher_reload_returns_config() {
        let (_file, path) = create_temp_config();
        let watcher = ConfigWatcher::new(path);
        let result = watcher.reload().await;
        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.global.log_level, "info");
    }

    #[tokio::test]
    async fn test_config_watcher_reload_nonexistent_returns_error() {
        let path = PathBuf::from("/nonexistent/config.toml");
        let watcher = ConfigWatcher::new(path);
        let result = watcher.reload().await;
        assert!(result.is_err());
    }
}
