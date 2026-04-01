//! Main daemon service
//!
//! This module provides the main service coordinator that integrates all components.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::config::{Config, ConfigLoader};
use crate::engine::EventDispatcher;
use crate::monitor::NetlinkMonitor;
use crate::monitor::NetworkEvent;
use crate::router::RouteManager;

use super::control::{ControlState, DaemonStatus};
use super::signals::{Signal, SignalHandler};
use super::systemd::SystemdNotify;

/// Main service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Service is initializing
    Initializing,
    /// Service is running
    Running,
    /// Service is reloading configuration
    Reloading,
    /// Service is stopping
    Stopping,
    /// Service is stopped
    Stopped,
}

/// Daemon service configuration
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Path to configuration file
    pub config_path: std::path::PathBuf,
    /// Control socket path
    pub socket_path: std::path::PathBuf,
    /// Enable configuration hot-reload
    pub enable_hot_reload: bool,
    /// Dry run mode (don't actually modify routes)
    pub dry_run: bool,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            config_path: std::path::PathBuf::from("/etc/nic-autoswitch/config.toml"),
            socket_path: std::path::PathBuf::from("/run/nic-autoswitch/control.sock"),
            enable_hot_reload: true,
            dry_run: false,
        }
    }
}

/// Main daemon service
pub struct DaemonService {
    /// Service configuration
    service_config: ServiceConfig,
    /// Configuration
    config: RwLock<Config>,
    /// Event dispatcher
    dispatcher: Arc<EventDispatcher>,
    /// Signal handler
    signal_handler: SignalHandler,
    /// Systemd notify
    systemd: SystemdNotify,
    /// Service state
    state: RwLock<ServiceState>,
    /// Start time
    start_time: RwLock<Option<Instant>>,
    /// Shutdown flag
    shutdown: RwLock<bool>,
    /// Netlink monitor for polling interface changes
    netlink_monitor: RwLock<Option<NetlinkMonitor>>,
}

impl DaemonService {
    /// Create a new daemon service (synchronous part)
    ///
    /// Call `init()` afterwards to complete async initialization
    /// (RouteManager connection, NetlinkMonitor setup).
    pub fn new(service_config: ServiceConfig) -> crate::Result<Self> {
        // Load configuration using convenience function
        let config = ConfigLoader::new(&service_config.config_path)?.load()?;

        // Create route manager stub (will be replaced in init())
        let route_manager = Arc::new(RouteManager::default());

        // Create event dispatcher
        let dispatcher = Arc::new(EventDispatcher::new(config.clone(), route_manager));

        Ok(Self {
            service_config,
            config: RwLock::new(config),
            dispatcher,
            signal_handler: SignalHandler::new()?,
            systemd: SystemdNotify::new(),
            state: RwLock::new(ServiceState::Initializing),
            start_time: RwLock::new(None),
            shutdown: RwLock::new(false),
            netlink_monitor: RwLock::new(None),
        })
    }

    /// Async initialization: connects RouteManager and starts NetlinkMonitor
    pub async fn init(&self) -> crate::Result<()> {
        let config = self.config.read().clone();
        let dry_run = self.service_config.dry_run || config.global.dry_run;

        // Create real RouteManager connection
        let route_manager =
            RouteManager::with_connection(config.global.table_id_start, dry_run).await?;

        // Replace the stub dispatcher's route manager
        let rm = Arc::new(route_manager);
        self.dispatcher.set_route_manager(rm.clone());

        // Initialize NetlinkMonitor
        let monitor = NetlinkMonitor::new().await?;
        *self.netlink_monitor.write() = Some(monitor);

        // TODO: Initialize NetworkManagerMonitor for SSID tracking (D-Bus)
        // This enables WifiConnected/WifiDisconnected events via zbus.
        // Requires NetworkManager running; should gracefully degrade if unavailable.
        // let nm_monitor = NetworkManagerMonitor::new().await?;
        // *self.nm_monitor.write() = Some(nm_monitor);

        info!(
            "Daemon initialized (dry_run={}, table_id_start={})",
            dry_run, config.global.table_id_start
        );

        Ok(())
    }

    /// Get current service state
    pub fn state(&self) -> ServiceState {
        *self.state.read()
    }

    /// Run the daemon service
    pub async fn run(&self) -> crate::Result<()> {
        info!("Starting nic-autoswitch daemon");
        *self.start_time.write() = Some(Instant::now());

        // Notify systemd we're starting
        self.systemd.notify_reloading();

        // Start event dispatcher
        self.dispatcher.start();
        *self.state.write() = ServiceState::Running;

        // Notify systemd we're ready
        self.systemd.notify_ready();
        info!("Daemon started and ready");

        // Main event loop
        let result = self.main_loop().await;

        // Shutdown
        *self.state.write() = ServiceState::Stopping;
        self.systemd.notify_stopping();
        self.dispatcher.stop();
        *self.state.write() = ServiceState::Stopped;

        info!("Daemon stopped");
        result
    }

    /// Main event loop
    async fn main_loop(&self) -> crate::Result<()> {
        let mut signal_rx = self.signal_handler.subscribe();

        // Watchdog ticker — only create when watchdog is enabled
        let mut watchdog_interval = self
            .systemd
            .is_watchdog_enabled()
            .then(|| tokio::time::interval(self.systemd.watchdog_interval()));

        // Network polling interval (every 5 seconds)
        let mut network_poll_interval = tokio::time::interval(std::time::Duration::from_secs(5));

        loop {
            tokio::select! {
                // Signal handling
                result = signal_rx.recv() => {
                    match result {
                        Ok(signal) => {
                            match signal {
                                Signal::Shutdown => {
                                    info!("Received shutdown signal");
                                    *self.shutdown.write() = true;
                                    break;
                                }
                                Signal::Reload => {
                                    info!("Received reload signal");
                                    self.reload_configuration();
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Signal channel closed");
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            warn!("Signal channel lagged");
                        }
                    }
                }

                // Network state polling
                _ = network_poll_interval.tick() => {
                    self.poll_network_changes().await;
                }

                // Watchdog keepalive — only ticks when watchdog is enabled
                _ = async {
                    match &mut watchdog_interval {
                        Some(interval) => interval.tick().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.systemd.watchdog_keepalive();
                    debug!("Watchdog keepalive sent");
                }
            }
        }

        Ok(())
    }

    /// Poll network interface changes and dispatch events
    ///
    /// Uses a guard pattern to ensure the monitor is always put back,
    /// even if polling or event handling panics.
    async fn poll_network_changes(&self) {
        // Take the monitor out to avoid holding a non-async lock across an await point
        let monitor = match self.netlink_monitor.write().take() {
            Some(m) => m,
            None => return,
        };

        // Guard: ensures monitor is put back when this scope exits
        struct MonitorGuard<'a> {
            slot: &'a RwLock<Option<crate::monitor::NetlinkMonitor>>,
            monitor: Option<crate::monitor::NetlinkMonitor>,
        }

        impl<'a> Drop for MonitorGuard<'a> {
            fn drop(&mut self) {
                if let Some(m) = self.monitor.take() {
                    *self.slot.write() = Some(m);
                }
            }
        }

        let mut guard = MonitorGuard {
            slot: &self.netlink_monitor,
            monitor: Some(monitor),
        };

        let events = match guard.monitor.as_mut().unwrap().poll_changes().await {
            Ok(events) => events,
            Err(e) => {
                warn!("Failed to poll network changes: {}", e);
                return; // guard's Drop puts monitor back
            }
        };

        for event in events {
            info!("Network change detected: {:?}", event);
            if let Err(e) = self.handle_network_event(event).await {
                warn!("Failed to handle network event: {}", e);
            }
        }
        // guard's Drop puts monitor back
    }

    /// Reload configuration
    fn reload_configuration(&self) {
        info!("Reloading configuration");
        *self.state.write() = ServiceState::Reloading;
        self.systemd.notify_reloading();

        // Reload configuration from file
        match ConfigLoader::new(&self.service_config.config_path).and_then(|loader| loader.load()) {
            Ok(config) => {
                *self.config.write() = config.clone();
                self.dispatcher.update_config(config);
                info!("Configuration reloaded successfully");
            }
            Err(e) => {
                error!("Failed to reload configuration: {}", e);
            }
        }

        *self.state.write() = ServiceState::Running;
        self.systemd.notify_ready();
    }

    /// Handle a network event
    pub async fn handle_network_event(&self, event: NetworkEvent) -> crate::Result<()> {
        debug!("Handling network event: {:?}", event);
        // SSID tracking is handled by EventDispatcher internally
        self.dispatcher.handle_event(&event).await
    }

    /// Check if shutdown is requested
    pub fn is_shutdown_requested(&self) -> bool {
        *self.shutdown.read()
    }

    /// Request shutdown
    pub fn request_shutdown(&self) {
        *self.shutdown.write() = true;
    }
}

/// Control state implementation for DaemonService
impl ControlState for DaemonService {
    fn get_status(&self) -> DaemonStatus {
        let start_time = self.start_time.read();
        let uptime_secs = start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);

        DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            state: format!("{:?}", self.state()),
            uptime_secs,
            active_routes: self.dispatcher.active_routes().len(),
        }
    }

    fn get_active_routes(&self) -> HashMap<String, serde_json::Value> {
        self.dispatcher
            .active_routes()
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect()
    }

    fn reload_config(&self) -> crate::Result<()> {
        let config = ConfigLoader::new(&self.service_config.config_path)?.load()?;
        *self.config.write() = config.clone();
        self.dispatcher.update_config(config);
        Ok(())
    }

    fn is_shutdown_requested(&self) -> bool {
        *self.shutdown.read()
    }

    fn request_shutdown(&self) {
        *self.shutdown.write() = true;
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

    fn create_temp_config() -> (NamedTempFile, std::path::PathBuf) {
        let mut file = NamedTempFile::new().unwrap();
        let content = r#"
[global]
log_level = "info"
table_id_start = 100

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
    fn test_service_state_debug() {
        assert!(format!("{:?}", ServiceState::Running).contains("Running"));
    }

    #[test]
    fn test_service_state_equality() {
        assert_eq!(ServiceState::Running, ServiceState::Running);
        assert_ne!(ServiceState::Running, ServiceState::Stopped);
        assert_ne!(ServiceState::Initializing, ServiceState::Reloading);
        assert_ne!(ServiceState::Stopping, ServiceState::Stopped);
    }

    #[test]
    fn test_service_config_default() {
        let config = ServiceConfig::default();
        assert!(config.enable_hot_reload);
        assert!(!config.dry_run);
    }

    #[test]
    fn test_daemon_status_creation() {
        let status = DaemonStatus {
            version: "0.1.0".to_string(),
            state: "Running".to_string(),
            uptime_secs: 100,
            active_routes: 5,
        };
        assert_eq!(status.version, "0.1.0");
        assert_eq!(status.active_routes, 5);
    }

    #[tokio::test]
    async fn test_daemon_service_new() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };

        let result = DaemonService::new(service_config);
        assert!(result.is_ok());

        let service = result.unwrap();
        assert_eq!(service.state(), ServiceState::Initializing);
    }

    #[tokio::test]
    async fn test_daemon_service_is_shutdown_requested() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };

        let service = DaemonService::new(service_config).unwrap();
        assert!(!service.is_shutdown_requested());
        service.request_shutdown();
        assert!(service.is_shutdown_requested());
    }

    // -------------------------------------------------------------------------
    // ControlState trait implementation tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_control_state_get_status_returns_info() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();

        let status = service.get_status();
        assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(status.state, "Initializing");
        assert_eq!(status.active_routes, 0);
    }

    #[tokio::test]
    async fn test_control_state_get_active_routes_returns_map() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();

        let routes = service.get_active_routes();
        assert!(routes.is_empty());
    }

    #[tokio::test]
    async fn test_control_state_reload_config_succeeds() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();

        let result = service.reload_config();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_control_state_reload_config_invalid_path_returns_error() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path.clone(),
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();

        // Drop temp file so path becomes invalid
        drop(_file);

        let result = service.reload_config();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_control_state_shutdown_request() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();

        assert!(!service.is_shutdown_requested());
        service.request_shutdown();
        assert!(service.is_shutdown_requested());
    }

    // -------------------------------------------------------------------------
    // Network event handling tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_handle_network_event_wifi_connected_tracks_ssid() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        service.dispatcher.start();

        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "TestWiFi".to_string(),
        };
        let result = service.handle_network_event(event).await;
        assert!(result.is_ok());
        assert_eq!(
            service.dispatcher.current_ssid("wlan0"),
            Some("TestWiFi".to_string())
        );
    }

    #[tokio::test]
    async fn test_handle_network_event_wifi_disconnected_removes_ssid() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        service.dispatcher.start();

        // Connect first
        let connect = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "TestWiFi".to_string(),
        };
        service.handle_network_event(connect).await.unwrap();
        assert_eq!(
            service.dispatcher.current_ssid("wlan0"),
            Some("TestWiFi".to_string())
        );

        // Disconnect
        let disconnect = NetworkEvent::WifiDisconnected {
            interface: "wlan0".to_string(),
            last_ssid: Some("TestWiFi".to_string()),
        };
        let result = service.handle_network_event(disconnect).await;
        assert!(result.is_ok());
        assert!(service.dispatcher.current_ssid("wlan0").is_none());
    }

    #[tokio::test]
    async fn test_handle_network_event_interface_change_noop() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        service.dispatcher.start();

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: crate::monitor::InterfaceChange::Up,
        };
        let result = service.handle_network_event(event).await;
        assert!(result.is_ok());
    }

    // -------------------------------------------------------------------------
    // ServiceState coverage
    // -------------------------------------------------------------------------

    #[test]
    fn test_service_state_variants() {
        let states = vec![
            ServiceState::Initializing,
            ServiceState::Running,
            ServiceState::Reloading,
            ServiceState::Stopping,
            ServiceState::Stopped,
        ];
        for state in &states {
            let debug = format!("{:?}", state);
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn test_service_config_custom() {
        let config = ServiceConfig {
            config_path: std::path::PathBuf::from("/custom/config.toml"),
            socket_path: std::path::PathBuf::from("/custom/control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        assert!(!config.enable_hot_reload);
        assert!(config.dry_run);
        assert_eq!(
            config.config_path,
            std::path::PathBuf::from("/custom/config.toml")
        );
        assert_eq!(
            config.socket_path,
            std::path::PathBuf::from("/custom/control.sock")
        );
    }

    #[tokio::test]
    async fn test_daemon_service_state_transitions() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        assert_eq!(service.state(), ServiceState::Initializing);

        // Dispatcher is not started yet, so state should remain Initializing
        assert_eq!(service.state(), ServiceState::Initializing);
    }

    #[tokio::test]
    async fn test_daemon_service_handle_address_changed() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        service.dispatcher.start();

        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec!["192.168.1.2/24".parse().unwrap()],
            removed: vec![],
        };
        let result = service.handle_network_event(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_daemon_service_handle_route_changed() {
        let (_file, path) = create_temp_config();
        let service_config = ServiceConfig {
            config_path: path,
            socket_path: std::path::PathBuf::from("/tmp/test-control.sock"),
            enable_hot_reload: false,
            dry_run: true,
        };
        let service = DaemonService::new(service_config).unwrap();
        service.dispatcher.start();

        let event = NetworkEvent::RouteChanged {
            interface: Some("eth0".to_string()),
            destination: "0.0.0.0/0".parse().unwrap(),
            gateway: None,
            added: true,
        };
        let result = service.handle_network_event(event).await;
        assert!(result.is_ok());
    }
}
