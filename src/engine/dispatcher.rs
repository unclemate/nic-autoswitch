//! Event dispatcher
//!
//! This module provides event handling and distribution logic.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use super::executor::{ActiveRoute, RuleExecutor};
use crate::config::Config;
use crate::monitor::{NetworkEvent, NetworkState};
use crate::router::RouteManager;

/// Event dispatcher state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatcherState {
    /// Dispatcher is initializing
    Initializing,
    /// Dispatcher is running
    Running,
    /// Dispatcher is stopping
    Stopping,
    /// Dispatcher is stopped
    Stopped,
}

/// Event dispatcher for handling network events
#[derive(Debug)]
pub struct EventDispatcher {
    /// Configuration
    config: Arc<RwLock<Config>>,
    /// Network state
    network_state: Arc<RwLock<NetworkState>>,
    /// Rule executor
    executor: Arc<RuleExecutor>,
    /// Current WiFi SSID per interface
    current_ssids: RwLock<HashMap<String, String>>,
    /// Interface name → sequential index for table ID calculation
    interface_indices: RwLock<HashMap<String, u8>>,
    /// Dispatcher state
    state: RwLock<DispatcherState>,
}

impl EventDispatcher {
    /// Create a new event dispatcher
    pub fn new(config: Config, route_manager: Arc<RouteManager>) -> Self {
        // Pre-assign indices for configured interfaces
        let mut interface_indices = HashMap::new();
        for (idx, name) in config.interfaces.keys().enumerate() {
            if idx < u8::MAX as usize {
                interface_indices.insert(name.clone(), idx as u8);
            }
        }

        Self {
            config: Arc::new(RwLock::new(config)),
            network_state: Arc::new(RwLock::new(NetworkState::new())),
            executor: Arc::new(RuleExecutor::new(route_manager)),
            current_ssids: RwLock::new(HashMap::new()),
            interface_indices: RwLock::new(interface_indices),
            state: RwLock::new(DispatcherState::Initializing),
        }
    }

    /// Get the current state
    pub fn state(&self) -> DispatcherState {
        *self.state.read()
    }

    /// Start the dispatcher
    pub fn start(&self) {
        let mut state = self.state.write();
        if *state == DispatcherState::Running {
            warn!("Dispatcher is already running");
            return;
        }
        *state = DispatcherState::Running;
        info!("Event dispatcher started");
    }

    /// Stop the dispatcher
    pub fn stop(&self) {
        let mut state = self.state.write();
        *state = DispatcherState::Stopping;
        info!("Event dispatcher stopping");

        // Clear all routes
        self.executor.clear_active_routes();

        *state = DispatcherState::Stopped;
        info!("Event dispatcher stopped");
    }

    /// Handle a network event
    pub async fn handle_event(&self, event: &NetworkEvent) -> crate::Result<()> {
        if *self.state.read() != DispatcherState::Running {
            debug!("Dispatcher not running, ignoring event: {:?}", event);
            return Ok(());
        }

        debug!("Handling event: {:?}", event);

        // Update network state
        self.network_state.write().apply_event(event)?;

        // Process event based on type
        match event {
            NetworkEvent::InterfaceChanged { interface, change } => {
                self.handle_interface_change(interface, *change).await?;
            }
            NetworkEvent::WifiConnected { interface, ssid } => {
                self.handle_wifi_connected(interface, ssid).await?;
            }
            NetworkEvent::WifiDisconnected {
                interface,
                last_ssid,
            } => {
                self.handle_wifi_disconnected(interface, last_ssid.as_deref())
                    .await?;
            }
            NetworkEvent::AddressChanged { interface, .. } => {
                self.handle_address_change(interface).await?;
            }
            NetworkEvent::RouteChanged { .. } => {
                // Route changes don't trigger rule updates
                debug!("Route changed event - no action needed");
            }
        }

        Ok(())
    }

    /// Handle interface state change
    async fn handle_interface_change(
        &self,
        interface: &str,
        change: crate::monitor::InterfaceChange,
    ) -> crate::Result<()> {
        debug!("Interface changed: {} ({:?})", interface, change);

        match change {
            crate::monitor::InterfaceChange::Down | crate::monitor::InterfaceChange::Removed => {
                self.handle_interface_down(interface).await?;
            }
            crate::monitor::InterfaceChange::Up => {
                let should_apply = {
                    let network_state = self.network_state.read();
                    if let Some(info) = network_state.get_interface(interface) {
                        if !info.is_up {
                            debug!("Interface {} is down, skipping", interface);
                            return Ok(());
                        }
                        let ssids = self.current_ssids.read();
                        !ssids.contains_key(interface)
                    } else {
                        false
                    }
                };

                if should_apply {
                    self.apply_default_rules(interface).await?;
                }
            }
            crate::monitor::InterfaceChange::Added => {
                debug!("Interface {} added, waiting for Up event", interface);
            }
        }

        Ok(())
    }

    /// Handle interface going down or being removed
    ///
    /// Removes all active routes whose `route_via` references the downed
    /// interface, along with the corresponding policy rules.
    async fn handle_interface_down(&self, interface: &str) -> crate::Result<()> {
        info!(
            "Interface {} went down, cleaning up dependent routes",
            interface
        );

        // Collect active routes that route via the downed interface
        let to_remove: Vec<(String, u32)> = {
            let active = self.executor.active_routes();
            active
                .iter()
                .filter(|(_, route)| route.interface == interface)
                .map(|(name, route)| (name.clone(), route.table_id))
                .collect()
        };

        if to_remove.is_empty() {
            debug!("No active routes depend on interface {}", interface);
            return Ok(());
        }

        // Load matching rules from config (both default and wifi profile rules)
        let rules_map: std::collections::HashMap<String, crate::config::RouteRule> = {
            let config = self.config.read();
            let mut map = std::collections::HashMap::new();
            for r in &config.routing.default_rules {
                map.insert(r.name.clone(), r.clone());
            }
            for profile in config.wifi_profiles.values() {
                for r in &profile.rules {
                    map.insert(r.name.clone(), r.clone());
                }
            }
            map
        };

        for (rule_name, table_id) in &to_remove {
            if let Some(rule) = rules_map.get(rule_name) {
                info!(
                    "Removing route '{}' (via {}) from table {}",
                    rule_name, interface, table_id
                );
                if let Err(e) = self.executor.remove_rule(rule, *table_id).await {
                    warn!(
                        "Failed to remove rule '{}' on interface down: {}",
                        rule_name, e
                    );
                }
            }
        }

        Ok(())
    }

    /// Handle WiFi connection
    async fn handle_wifi_connected(&self, interface: &str, ssid: &str) -> crate::Result<()> {
        info!("WiFi connected: {} -> {}", interface, ssid);

        // Store current SSID
        self.current_ssids
            .write()
            .insert(interface.to_string(), ssid.to_string());

        self.apply_rules_for_interface(interface, Some(ssid)).await
    }

    /// Handle WiFi disconnection
    async fn handle_wifi_disconnected(
        &self,
        interface: &str,
        _last_ssid: Option<&str>,
    ) -> crate::Result<()> {
        info!("WiFi disconnected: {}", interface);

        // Remove SSID tracking
        self.current_ssids.write().remove(interface);

        // Apply default rules
        self.apply_default_rules(interface).await?;

        Ok(())
    }

    /// Handle address change
    async fn handle_address_change(&self, interface: &str) -> crate::Result<()> {
        debug!("Address changed on interface: {}", interface);

        let ssid = {
            let ssids = self.current_ssids.read();
            ssids.get(interface).cloned()
        };

        self.apply_rules_for_interface(interface, ssid.as_deref())
            .await
    }

    /// Apply rules for an interface based on its current SSID (if any).
    ///
    /// If a WiFi profile matches, SSID, apply its rules; otherwise fall back to default rules.
    async fn apply_rules_for_interface(
        &self,
        interface: &str,
        ssid: Option<&str>,
    ) -> crate::Result<()> {
        if let Some(ssid) = ssid {
            let profile = {
                let config = self.config.read();
                config.wifi_profiles.get(ssid).cloned()
            };

            if let Some(profile) = profile {
                let table_id = self.get_table_id_for_interface(interface);
                self.executor
                    .apply_wifi_profile(&profile, 0, table_id)
                    .await?;
                info!(
                    "Applied WiFi profile '{}' for interface {}",
                    ssid, interface
                );
                return Ok(());
            }
        }

        // No matching profile or no SSID → apply defaults
        self.apply_default_rules(interface).await
    }

    /// Apply default routing rules
    ///
    /// Default rules are global and should only exist once.  Rules that
    /// are already tracked as active are skipped to prevent duplicates
    /// when multiple configured interfaces are up simultaneously.
    async fn apply_default_rules(&self, interface: &str) -> crate::Result<()> {
        // Clone rules to avoid holding lock across await
        let rules = {
            let config = self.config.read();
            config.routing.default_rules.clone()
        };

        if rules.is_empty() {
            debug!("No default rules to apply");
            return Ok(());
        }

        let table_id = self.get_table_id_for_interface(interface);

        for rule in &rules {
            // Skip rules already applied to any table (prevents duplicates)
            if self.executor.is_route_active(&rule.name) {
                debug!(
                    "Default rule '{}' already active, skipping duplicate",
                    rule.name
                );
                continue;
            }
            if let Err(e) = self.executor.apply_rule(rule, 0, table_id).await {
                warn!("Failed to apply default rule '{}': {}", rule.name, e);
            }
        }

        info!(
            "Applied default rules for interface {} (table {})",
            interface, table_id
        );
        Ok(())
    }

    /// Get table ID for an interface, assigning a unique index if new.
    fn get_table_id_for_interface(&self, interface: &str) -> u32 {
        let index = {
            let mut indices = self.interface_indices.write();
            if let Some(&idx) = indices.get(interface) {
                idx
            } else {
                let idx = indices.len() as u8;
                indices.insert(interface.to_string(), idx);
                debug!("Assigned table index {} to interface {}", idx, interface);
                idx
            }
        };
        self.executor.route_manager().table_id_v4(index)
    }

    /// Update configuration (preserves existing RouteManager connection)
    ///
    /// Rebuilds the interface→index mapping from the new configuration.
    /// Interfaces that existed before keep their original indices; new ones
    /// get appended. Removed interfaces' indices are freed but not reused.
    pub fn update_config(&self, config: Config) {
        let mut new_indices = HashMap::new();
        let existing = self.interface_indices.read();
        let mut next_idx = existing.len() as u8;

        for name in config.interfaces.keys() {
            if let Some(&idx) = existing.get(name) {
                // Preserve existing index
                new_indices.insert(name.clone(), idx);
            } else if next_idx < u8::MAX {
                new_indices.insert(name.clone(), next_idx);
                next_idx += 1;
            }
        }
        drop(existing);

        *self.interface_indices.write() = new_indices;
        *self.config.write() = config;
        info!("Configuration updated in dispatcher (interface indices rebuilt)");
    }

    /// Set a new route manager (replaces the one in executor)
    pub fn set_route_manager(&self, rm: Arc<RouteManager>) {
        self.executor.set_route_manager(rm);
        info!("Route manager updated in dispatcher");
    }

    /// Get current SSID for interface
    pub fn current_ssid(&self, interface: &str) -> Option<String> {
        self.current_ssids.read().get(interface).cloned()
    }

    /// Apply initial routing rules based on current interface state.
    ///
    /// Called once at daemon startup to populate `NetworkState` with
    /// currently-active interfaces and apply default routing rules for
    /// every interface that is both configured and up.
    pub async fn apply_initial_state(
        &self,
        interfaces: &[crate::monitor::InterfaceInfo],
    ) -> crate::Result<()> {
        // 1. Populate NetworkState with all known interfaces
        {
            let mut state = self.network_state.write();
            for info in interfaces {
                state.update_interface(info.clone());
            }
        }

        // 2. Clean up stale tables from previous daemon runs
        self.cleanup_stale_tables().await?;

        // 3. Collect names of configured-and-up interfaces (lock scope is tiny)
        let names: Vec<String> = {
            let config = self.config.read();
            interfaces
                .iter()
                .filter(|info| info.is_up && config.interfaces.contains_key(&info.name))
                .map(|info| info.name.clone())
                .collect()
        };

        // 4. Apply default rules for each qualifying interface (no lock held across await)
        for name in &names {
            info!("Applying initial rules for configured interface: {}", name);
            if let Err(e) = self.apply_default_rules(name).await {
                tracing::warn!("Failed to apply initial rules for {}: {}", name, e);
            }
        }

        Ok(())
    }

    /// Clean up stale routing tables and policy rules from previous daemon runs.
    ///
    /// Iterates all table IDs that could have been assigned in previous runs
    /// (base..base+MAX_INTERFACES for IPv4, 200..200+MAX_INTERFACES for IPv6)
    /// and flushes both routes and policy rules. This ensures a clean slate
    /// before applying the current configuration.
    async fn cleanup_stale_tables(&self) -> crate::Result<()> {
        const MAX_INTERFACES: u32 = 32; // generous upper bound

        // Clone Arc to release parking_lot lock before .await
        let rm = Arc::clone(&*self.executor.route_manager());
        let base = rm.table_id_v4(0); // e.g. 100

        let mut total_cleaned = 0usize;

        for idx in 0..MAX_INTERFACES {
            let table_v4 = base + idx;
            let table_v6 = 200 + idx;

            for table_id in [table_v4, table_v6] {
                match rm.flush_table_complete(table_id).await {
                    Ok(count) if count > 0 => {
                        total_cleaned += count;
                        info!("Cleaned up {} stale entries from table {}", count, table_id);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        debug!("Failed to flush table {} during cleanup: {}", table_id, e);
                    }
                }
            }
        }

        if total_cleaned > 0 {
            info!(
                "Startup cleanup: removed {} total stale entries across custom tables",
                total_cleaned
            );
        } else {
            debug!("No stale routing entries found during startup cleanup");
        }

        Ok(())
    }

    /// Get network state
    pub fn network_state(&self) -> NetworkState {
        self.network_state.read().clone()
    }

    /// Get active routes
    pub fn active_routes(&self) -> HashMap<String, ActiveRoute> {
        self.executor.active_routes()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        GlobalConfig, InterfaceConfig, InterfaceType as ConfigInterfaceType, RouteRule, RouteVia,
        RoutingConfig, WifiProfile,
    };
    use crate::monitor::{InterfaceChange, InterfaceInfo, InterfaceType as MonitorInterfaceType};
    use std::collections::HashMap;

    fn create_test_config() -> Config {
        let mut interfaces = HashMap::new();
        interfaces.insert(
            "eth0".to_string(),
            InterfaceConfig {
                interface_type: ConfigInterfaceType::Lan,
                match_by: crate::config::MatchBy::Name {
                    name: "eth0".to_string(),
                },
                priority: 10,
            },
        );

        Config {
            global: GlobalConfig::default(),
            interfaces,
            wifi_profiles: HashMap::new(),
            routing: RoutingConfig::default(),
        }
    }

    #[test]
    fn test_dispatcher_state_initialization() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        assert_eq!(dispatcher.state(), DispatcherState::Initializing);
    }

    #[test]
    fn test_dispatcher_start_changes_state_to_running() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        dispatcher.start();
        assert_eq!(dispatcher.state(), DispatcherState::Running);
    }

    #[test]
    fn test_dispatcher_stop_changes_state_to_stopped() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        dispatcher.start();
        dispatcher.stop();
        assert_eq!(dispatcher.state(), DispatcherState::Stopped);
    }

    #[test]
    fn test_current_ssid_returns_none_when_not_connected() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        assert!(dispatcher.current_ssid("wlan0").is_none());
    }

    #[tokio::test]
    async fn test_handle_event_when_stopped_returns_early() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        // Dispatcher is in Initializing state, not Running
        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };

        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_config_updates_config() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        let mut new_config = create_test_config();
        new_config.global.log_level = "debug".to_string();

        dispatcher.update_config(new_config);
    }

    #[test]
    fn test_network_state_returns_state() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        let state = dispatcher.network_state();
        assert!(state.interfaces().is_empty());
    }

    #[test]
    fn test_active_routes_returns_empty_initially() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        let routes = dispatcher.active_routes();
        assert!(routes.is_empty());
    }

    #[tokio::test]
    async fn test_handle_interface_down_skips_rules() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Add an interface that is down
        let mut state = NetworkState::new();
        let mut info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        info.is_up = false;
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Down,
        };

        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_dispatcher_state_debug() {
        assert_eq!(format!("{:?}", DispatcherState::Running), "Running");
        assert_eq!(format!("{:?}", DispatcherState::Stopped), "Stopped");
    }

    #[test]
    fn test_dispatcher_state_equality() {
        assert_eq!(DispatcherState::Running, DispatcherState::Running);
        assert_ne!(DispatcherState::Running, DispatcherState::Stopped);
    }

    // -------------------------------------------------------------------------
    // Interface change handling tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_handle_interface_change_up_with_no_wifi_applies_default_rules() {
        let mut config = create_test_config();
        config.routing.default_rules = vec![RouteRule {
            name: "default-rule".to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: crate::config::RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 10000,
        }];
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Set up network state with an interface that is up
        let mut state = NetworkState::new();
        let mut info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        info.is_up = true;
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert_eq!(dispatcher.active_routes().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_interface_change_up_with_active_wifi_skips_default() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Set up network state with interface up
        let mut state = NetworkState::new();
        let mut info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        info.is_up = true;
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        // Set active WiFi SSID so default rules are skipped
        dispatcher
            .current_ssids
            .write()
            .insert("eth0".to_string(), "TestWiFi".to_string());

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert!(dispatcher.active_routes().is_empty());
    }

    #[tokio::test]
    async fn test_handle_interface_change_unknown_interface_skips() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // No interfaces in network state
        let event = NetworkEvent::InterfaceChanged {
            interface: "unknown0".to_string(),
            change: InterfaceChange::Up,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert!(dispatcher.active_routes().is_empty());
    }

    #[tokio::test]
    async fn test_handle_route_changed_event_noop() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        let event = NetworkEvent::RouteChanged {
            interface: Some("eth0".to_string()),
            destination: "0.0.0.0/0".parse().unwrap(),
            gateway: None,
            added: true,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert!(dispatcher.active_routes().is_empty());
    }

    #[tokio::test]
    async fn test_apply_default_rules_with_empty_rules_returns_early() {
        let config = create_test_config(); // No default rules
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        let mut state = NetworkState::new();
        let mut info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        info.is_up = true;
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert!(dispatcher.active_routes().is_empty());
    }

    #[tokio::test]
    async fn test_handle_address_change_without_wifi_applies_default() {
        let mut config = create_test_config();
        config.routing.default_rules = vec![RouteRule {
            name: "default-addr".to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: crate::config::RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 10000,
        }];
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Set up network state
        let mut state = NetworkState::new();
        let info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec!["192.168.1.1/24".parse().unwrap()],
            removed: vec![],
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_dispatcher_start_already_running_warns() {
        let config = create_test_config();
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);

        dispatcher.start();
        assert_eq!(dispatcher.state(), DispatcherState::Running);
        // Start again — should warn but remain Running
        dispatcher.start();
        assert_eq!(dispatcher.state(), DispatcherState::Running);
    }

    #[tokio::test]
    async fn test_handle_wifi_connected_with_profile_applies_rules() {
        let mut config = create_test_config();
        config.wifi_profiles.insert(
            "TestWiFi".to_string(),
            WifiProfile {
                interface: "wlan0".to_string(),
                rules: vec![RouteRule {
                    name: "wifi-rule".to_string(),
                    match_on: crate::config::MatchOn::Cidr {
                        cidr: "10.0.0.0/8".parse().unwrap(),
                    },
                    route_via: RouteVia {
                        interface: "wlan0".to_string(),
                    },
                    priority: 100,
                }],
            },
        );
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "TestWiFi".to_string(),
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert_eq!(
            dispatcher.current_ssid("wlan0"),
            Some("TestWiFi".to_string())
        );
        assert_eq!(dispatcher.active_routes().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_wifi_connected_without_profile_applies_default() {
        let mut config = create_test_config();
        config.routing.default_rules = vec![RouteRule {
            name: "default-rule".to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "wlan0".to_string(),
            },
            priority: 10000,
        }];
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "UnknownWiFi".to_string(),
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert_eq!(
            dispatcher.current_ssid("wlan0"),
            Some("UnknownWiFi".to_string())
        );
        assert_eq!(dispatcher.active_routes().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_wifi_disconnected_removes_ssid_and_applies_default() {
        let mut config = create_test_config();
        config.routing.default_rules = vec![RouteRule {
            name: "default-rule".to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "wlan0".to_string(),
            },
            priority: 10000,
        }];
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // First connect to set SSID
        dispatcher
            .current_ssids
            .write()
            .insert("wlan0".to_string(), "TestWiFi".to_string());

        let event = NetworkEvent::WifiDisconnected {
            interface: "wlan0".to_string(),
            last_ssid: Some("TestWiFi".to_string()),
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert!(dispatcher.current_ssid("wlan0").is_none());
        assert_eq!(dispatcher.active_routes().len(), 1);
    }

    #[tokio::test]
    async fn test_handle_address_change_with_wifi_profile_reapplies() {
        let mut config = create_test_config();
        config.wifi_profiles.insert(
            "TestWiFi".to_string(),
            WifiProfile {
                interface: "wlan0".to_string(),
                rules: vec![RouteRule {
                    name: "wifi-rule".to_string(),
                    match_on: crate::config::MatchOn::Cidr {
                        cidr: "10.0.0.0/8".parse().unwrap(),
                    },
                    route_via: RouteVia {
                        interface: "wlan0".to_string(),
                    },
                    priority: 100,
                }],
            },
        );
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Set up network state
        let mut state = NetworkState::new();
        let info = InterfaceInfo::new("wlan0".to_string(), MonitorInterfaceType::Wlan);
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        // Set active WiFi SSID so address change re-applies WiFi profile
        dispatcher
            .current_ssids
            .write()
            .insert("wlan0".to_string(), "TestWiFi".to_string());

        let event = NetworkEvent::AddressChanged {
            interface: "wlan0".to_string(),
            added: vec!["192.168.1.2/24".parse().unwrap()],
            removed: vec![],
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        assert_eq!(dispatcher.active_routes().len(), 1);
    }

    #[tokio::test]
    async fn test_apply_default_rules_with_failing_rule_logs_warning() {
        let mut config = create_test_config();
        // Domain-based rule will fail validation in RuleOperator
        config.routing.default_rules = vec![RouteRule {
            name: "bad-rule".to_string(),
            match_on: crate::config::MatchOn::Domain {
                domain: "example.com".to_string(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        }];
        let manager = Arc::new(RouteManager::default());
        let dispatcher = EventDispatcher::new(config, manager);
        dispatcher.start();

        // Set up network state with interface up
        let mut state = NetworkState::new();
        let mut info = InterfaceInfo::new("eth0".to_string(), MonitorInterfaceType::Lan);
        info.is_up = true;
        state.update_interface(info);
        *dispatcher.network_state.write() = state;

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        let result = dispatcher.handle_event(&event).await;
        assert!(result.is_ok());
        // Rule failed but event handling succeeded
        assert!(dispatcher.active_routes().is_empty());
    }
}
