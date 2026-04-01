//! Network state management
//!
//! This module defines the network state structures and provides
//! methods for tracking interface states and WiFi connections.

use ipnetwork::IpNetwork;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::events::{InterfaceChange, NetworkEvent};

/// Interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceType {
    /// Wired LAN
    Lan,
    /// Wireless WLAN
    Wlan,
    /// VPN tunnel
    Vpn,
}

/// Interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    /// Interface name (e.g., "eth0", "wlan0")
    pub name: String,
    /// Interface type
    pub interface_type: InterfaceType,
    /// IP addresses assigned to this interface
    pub addresses: Vec<IpNetwork>,
    /// Whether interface is up
    pub is_up: bool,
    /// Current SSID (for WLAN interfaces)
    pub ssid: Option<String>,
}

impl InterfaceInfo {
    /// Create a new interface info
    pub fn new(name: String, interface_type: InterfaceType) -> Self {
        Self {
            name,
            interface_type,
            addresses: Vec::new(),
            is_up: false,
            ssid: None,
        }
    }

    /// Check if interface has IPv4 address
    pub fn has_ipv4(&self) -> bool {
        self.addresses.iter().any(|a| a.is_ipv4())
    }

    /// Check if interface has IPv6 address
    pub fn has_ipv6(&self) -> bool {
        self.addresses.iter().any(|a| a.is_ipv6())
    }

    /// Get IPv4 addresses
    pub fn ipv4_addresses(&self) -> Vec<IpNetwork> {
        self.addresses
            .iter()
            .filter(|a| a.is_ipv4())
            .copied()
            .collect()
    }

    /// Get IPv6 addresses
    pub fn ipv6_addresses(&self) -> Vec<IpNetwork> {
        self.addresses
            .iter()
            .filter(|a| a.is_ipv6())
            .copied()
            .collect()
    }
}

/// Network state
#[derive(Debug, Clone, Default)]
pub struct NetworkState {
    /// Interface states
    interfaces: HashMap<String, InterfaceInfo>,
}

impl NetworkState {
    /// Create a new network state
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
        }
    }

    /// Get all interfaces
    pub fn interfaces(&self) -> &HashMap<String, InterfaceInfo> {
        &self.interfaces
    }

    /// Get a specific interface
    pub fn get_interface(&self, name: &str) -> Option<&InterfaceInfo> {
        self.interfaces.get(name)
    }

    /// Check if interface exists
    pub fn has_interface(&self, name: &str) -> bool {
        self.interfaces.contains_key(name)
    }

    /// Add or update an interface
    pub fn update_interface(&mut self, info: InterfaceInfo) {
        self.interfaces.insert(info.name.clone(), info);
    }

    /// Remove an interface
    pub fn remove_interface(&mut self, name: &str) -> Option<InterfaceInfo> {
        self.interfaces.remove(name)
    }

    /// Get all active (up) interfaces
    pub fn active_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.interfaces.values().filter(|i| i.is_up).collect()
    }

    /// Get all WLAN interfaces
    pub fn wlan_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.interfaces
            .values()
            .filter(|i| i.interface_type == InterfaceType::Wlan)
            .collect()
    }

    /// Get all LAN interfaces
    pub fn lan_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.interfaces
            .values()
            .filter(|i| i.interface_type == InterfaceType::Lan)
            .collect()
    }

    /// Apply a network event and update state
    pub fn apply_event(&mut self, event: &NetworkEvent) -> crate::Result<()> {
        match event {
            NetworkEvent::InterfaceChanged { interface, change } => {
                self.apply_interface_change(interface, *change)?;
            }
            NetworkEvent::WifiConnected { interface, ssid } => {
                self.apply_wifi_connected(interface, ssid)?;
            }
            NetworkEvent::WifiDisconnected { interface, .. } => {
                self.apply_wifi_disconnected(interface)?;
            }
            NetworkEvent::AddressChanged {
                interface,
                added,
                removed,
            } => {
                self.apply_address_change(interface, added, removed)?;
            }
            NetworkEvent::RouteChanged { .. } => {
                // Route changes don't affect network state
            }
        }
        Ok(())
    }

    fn apply_interface_change(
        &mut self,
        interface: &str,
        change: InterfaceChange,
    ) -> crate::Result<()> {
        match change {
            InterfaceChange::Up => {
                if let Some(info) = self.interfaces.get_mut(interface) {
                    info.is_up = true;
                }
            }
            InterfaceChange::Down => {
                if let Some(info) = self.interfaces.get_mut(interface) {
                    info.is_up = false;
                }
            }
            InterfaceChange::Added => {
                // Interface added but not configured yet - nothing to do
            }
            InterfaceChange::Removed => {
                self.interfaces.remove(interface);
            }
        }
        Ok(())
    }

    fn apply_wifi_connected(&mut self, interface: &str, ssid: &str) -> crate::Result<()> {
        if let Some(info) = self.interfaces.get_mut(interface) {
            info.ssid = Some(ssid.to_string());
            info.is_up = true;
        }
        Ok(())
    }

    fn apply_wifi_disconnected(&mut self, interface: &str) -> crate::Result<()> {
        if let Some(info) = self.interfaces.get_mut(interface) {
            info.ssid = None;
        }
        Ok(())
    }

    fn apply_address_change(
        &mut self,
        interface: &str,
        added: &[IpNetwork],
        removed: &[IpNetwork],
    ) -> crate::Result<()> {
        if let Some(info) = self.interfaces.get_mut(interface) {
            // Remove old addresses
            info.addresses.retain(|addr| !removed.contains(addr));
            // Add new addresses
            for addr in added {
                if !info.addresses.contains(addr) {
                    info.addresses.push(*addr);
                }
            }
        }
        Ok(())
    }
}

/// Thread-safe network state wrapper
pub struct SharedNetworkState {
    inner: RwLock<NetworkState>,
}

impl SharedNetworkState {
    /// Create a new shared network state
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(NetworkState::new()),
        }
    }

    /// Get a read lock
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, NetworkState> {
        self.inner.read()
    }

    /// Get a write lock
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, NetworkState> {
        self.inner.write()
    }

    /// Apply an event
    pub fn apply_event(&self, event: &NetworkEvent) -> crate::Result<()> {
        self.write().apply_event(event)
    }
}

impl Default for SharedNetworkState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_interface(name: &str, interface_type: InterfaceType) -> InterfaceInfo {
        InterfaceInfo::new(name.to_string(), interface_type)
    }

    fn create_test_address() -> IpNetwork {
        "192.168.1.100/24".parse().unwrap()
    }

    // -------------------------------------------------------------------------
    // InterfaceInfo Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_info_new_creates_interface() {
        let info = InterfaceInfo::new("eth0".to_string(), InterfaceType::Lan);
        assert_eq!(info.name, "eth0");
        assert_eq!(info.interface_type, InterfaceType::Lan);
        assert!(info.addresses.is_empty());
        assert!(!info.is_up);
        assert!(info.ssid.is_none());
    }

    #[test]
    fn test_interface_info_has_ipv4_returns_true_for_ipv4() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("192.168.1.1/24".parse().unwrap());
        assert!(info.has_ipv4());
    }

    #[test]
    fn test_interface_info_has_ipv4_returns_false_for_no_ipv4() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("2001:db8::1/64".parse().unwrap());
        assert!(!info.has_ipv4());
    }

    #[test]
    fn test_interface_info_has_ipv6_returns_true_for_ipv6() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("2001:db8::1/64".parse().unwrap());
        assert!(info.has_ipv6());
    }

    #[test]
    fn test_interface_info_has_ipv6_returns_false_for_no_ipv6() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("192.168.1.1/24".parse().unwrap());
        assert!(!info.has_ipv6());
    }

    #[test]
    fn test_interface_info_ipv4_addresses_filters_correctly() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("192.168.1.1/24".parse().unwrap());
        info.addresses.push("2001:db8::1/64".parse().unwrap());
        let ipv4 = info.ipv4_addresses();
        assert_eq!(ipv4.len(), 1);
        assert!(ipv4[0].is_ipv4());
    }

    #[test]
    fn test_interface_info_ipv6_addresses_filters_correctly() {
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("192.168.1.1/24".parse().unwrap());
        info.addresses.push("2001:db8::1/64".parse().unwrap());
        let ipv6 = info.ipv6_addresses();
        assert_eq!(ipv6.len(), 1);
        assert!(ipv6[0].is_ipv6());
    }

    // -------------------------------------------------------------------------
    // NetworkState Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_network_state_new_creates_empty_state() {
        let state = NetworkState::new();
        assert!(state.interfaces().is_empty());
    }

    #[test]
    fn test_network_state_update_interface_adds_interface() {
        let mut state = NetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.update_interface(info);
        assert!(state.has_interface("eth0"));
    }

    #[test]
    fn test_network_state_remove_interface_removes_interface() {
        let mut state = NetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.update_interface(info);
        let removed = state.remove_interface("eth0");
        assert!(removed.is_some());
        assert!(!state.has_interface("eth0"));
    }

    #[test]
    fn test_network_state_get_interface_returns_interface() {
        let mut state = NetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.update_interface(info);
        let retrieved = state.get_interface("eth0");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "eth0");
    }

    #[test]
    fn test_network_state_active_interfaces_returns_only_up() {
        let mut state = NetworkState::new();
        let mut eth0 = create_test_interface("eth0", InterfaceType::Lan);
        eth0.is_up = true;
        let eth1 = create_test_interface("eth1", InterfaceType::Lan);
        state.update_interface(eth0);
        state.update_interface(eth1);
        let active = state.active_interfaces();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "eth0");
    }

    #[test]
    fn test_network_state_wlan_interfaces_returns_only_wlan() {
        let mut state = NetworkState::new();
        let eth0 = create_test_interface("eth0", InterfaceType::Lan);
        let wlan0 = create_test_interface("wlan0", InterfaceType::Wlan);
        state.update_interface(eth0);
        state.update_interface(wlan0);
        let wlans = state.wlan_interfaces();
        assert_eq!(wlans.len(), 1);
        assert_eq!(wlans[0].name, "wlan0");
    }

    #[test]
    fn test_network_state_lan_interfaces_returns_only_lan() {
        let mut state = NetworkState::new();
        let eth0 = create_test_interface("eth0", InterfaceType::Lan);
        let wlan0 = create_test_interface("wlan0", InterfaceType::Wlan);
        state.update_interface(eth0);
        state.update_interface(wlan0);
        let lans = state.lan_interfaces();
        assert_eq!(lans.len(), 1);
        assert_eq!(lans[0].name, "eth0");
    }

    // -------------------------------------------------------------------------
    // apply_event Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_apply_event_interface_up_sets_is_up() {
        let mut state = NetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.update_interface(info);

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        state.apply_event(&event).unwrap();

        assert!(state.get_interface("eth0").unwrap().is_up);
    }

    #[test]
    fn test_apply_event_interface_down_clears_is_up() {
        let mut state = NetworkState::new();
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.is_up = true;
        state.update_interface(info);

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Down,
        };
        state.apply_event(&event).unwrap();

        assert!(!state.get_interface("eth0").unwrap().is_up);
    }

    #[test]
    fn test_apply_event_interface_removed_removes_interface() {
        let mut state = NetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.update_interface(info);

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Removed,
        };
        state.apply_event(&event).unwrap();

        assert!(!state.has_interface("eth0"));
    }

    #[test]
    fn test_apply_event_wifi_connected_sets_ssid() {
        let mut state = NetworkState::new();
        let info = create_test_interface("wlan0", InterfaceType::Wlan);
        state.update_interface(info);

        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "MyWiFi".to_string(),
        };
        state.apply_event(&event).unwrap();

        let interface = state.get_interface("wlan0").unwrap();
        assert_eq!(interface.ssid, Some("MyWiFi".to_string()));
        assert!(interface.is_up);
    }

    #[test]
    fn test_apply_event_wifi_disconnected_clears_ssid() {
        let mut state = NetworkState::new();
        let mut info = create_test_interface("wlan0", InterfaceType::Wlan);
        info.ssid = Some("MyWiFi".to_string());
        info.is_up = true;
        state.update_interface(info);

        let event = NetworkEvent::WifiDisconnected {
            interface: "wlan0".to_string(),
            last_ssid: Some("MyWiFi".to_string()),
        };
        state.apply_event(&event).unwrap();

        let interface = state.get_interface("wlan0").unwrap();
        assert!(interface.ssid.is_none());
    }

    #[test]
    fn test_apply_event_address_changed_updates_addresses() {
        let mut state = NetworkState::new();
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("10.0.0.1/24".parse().unwrap());
        state.update_interface(info);

        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec![create_test_address()],
            removed: vec!["10.0.0.1/24".parse().unwrap()],
        };
        state.apply_event(&event).unwrap();

        let interface = state.get_interface("eth0").unwrap();
        assert_eq!(interface.addresses.len(), 1);
        assert_eq!(interface.addresses[0], create_test_address());
    }

    // -------------------------------------------------------------------------
    // SharedNetworkState Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_shared_network_state_read_write() {
        let state = SharedNetworkState::new();
        {
            let mut w = state.write();
            let info = create_test_interface("eth0", InterfaceType::Lan);
            w.update_interface(info);
        }
        {
            let r = state.read();
            assert!(r.has_interface("eth0"));
        }
    }

    #[test]
    fn test_shared_network_state_apply_event() {
        let state = SharedNetworkState::new();
        let info = create_test_interface("eth0", InterfaceType::Lan);
        state.write().update_interface(info);

        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        state.apply_event(&event).unwrap();

        let r = state.read();
        assert!(r.get_interface("eth0").unwrap().is_up);
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_network_state_default_is_empty() {
        let state = NetworkState::default();
        assert!(state.interfaces().is_empty());
    }

    #[test]
    fn test_shared_network_state_default() {
        let state = SharedNetworkState::default();
        assert!(state.read().interfaces().is_empty());
    }

    #[test]
    fn test_apply_event_interface_added_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Added,
        };
        state.apply_event(&event).unwrap();
        // Added doesn't modify state if interface not present
        assert!(!state.has_interface("eth0"));
    }

    #[test]
    fn test_apply_event_interface_up_unknown_interface_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::InterfaceChanged {
            interface: "unknown0".to_string(),
            change: InterfaceChange::Up,
        };
        state.apply_event(&event).unwrap();
        assert!(!state.has_interface("unknown0"));
    }

    #[test]
    fn test_apply_event_interface_down_unknown_interface_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::InterfaceChanged {
            interface: "unknown0".to_string(),
            change: InterfaceChange::Down,
        };
        state.apply_event(&event).unwrap();
    }

    #[test]
    fn test_apply_event_wifi_connected_unknown_interface_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::WifiConnected {
            interface: "unknown0".to_string(),
            ssid: "Test".to_string(),
        };
        state.apply_event(&event).unwrap();
        assert!(!state.has_interface("unknown0"));
    }

    #[test]
    fn test_apply_event_wifi_disconnected_unknown_interface_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::WifiDisconnected {
            interface: "unknown0".to_string(),
            last_ssid: None,
        };
        state.apply_event(&event).unwrap();
    }

    #[test]
    fn test_apply_event_address_changed_unknown_interface_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::AddressChanged {
            interface: "unknown0".to_string(),
            added: vec!["10.0.0.1/24".parse().unwrap()],
            removed: vec![],
        };
        state.apply_event(&event).unwrap();
    }

    #[test]
    fn test_apply_event_route_changed_noop() {
        let mut state = NetworkState::new();
        let event = NetworkEvent::RouteChanged {
            interface: Some("eth0".to_string()),
            destination: "0.0.0.0/0".parse().unwrap(),
            gateway: None,
            added: true,
        };
        state.apply_event(&event).unwrap();
        // RouteChanged does not affect state
        assert!(state.interfaces().is_empty());
    }

    #[test]
    fn test_remove_interface_nonexistent_returns_none() {
        let mut state = NetworkState::new();
        let result = state.remove_interface("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_get_interface_nonexistent_returns_none() {
        let state = NetworkState::new();
        assert!(state.get_interface("nonexistent").is_none());
    }

    #[test]
    fn test_active_interfaces_all_down_returns_empty() {
        let mut state = NetworkState::new();
        state.update_interface(create_test_interface("eth0", InterfaceType::Lan));
        state.update_interface(create_test_interface("wlan0", InterfaceType::Wlan));
        assert!(state.active_interfaces().is_empty());
    }

    #[test]
    fn test_wlan_interfaces_empty_when_none() {
        let mut state = NetworkState::new();
        state.update_interface(create_test_interface("eth0", InterfaceType::Lan));
        assert!(state.wlan_interfaces().is_empty());
    }

    #[test]
    fn test_lan_interfaces_empty_when_none() {
        let mut state = NetworkState::new();
        state.update_interface(create_test_interface("wlan0", InterfaceType::Wlan));
        assert!(state.lan_interfaces().is_empty());
    }

    #[test]
    fn test_apply_event_address_add_duplicate_ignored() {
        let mut state = NetworkState::new();
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("10.0.0.1/24".parse().unwrap());
        state.update_interface(info);

        // Add same address again
        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec!["10.0.0.1/24".parse().unwrap()],
            removed: vec![],
        };
        state.apply_event(&event).unwrap();

        let iface = state.get_interface("eth0").unwrap();
        assert_eq!(iface.addresses.len(), 1); // No duplicate
    }

    #[test]
    fn test_apply_event_address_remove_nonexistent_noop() {
        let mut state = NetworkState::new();
        let mut info = create_test_interface("eth0", InterfaceType::Lan);
        info.addresses.push("10.0.0.1/24".parse().unwrap());
        state.update_interface(info);

        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec![],
            removed: vec!["192.168.1.1/24".parse().unwrap()],
        };
        state.apply_event(&event).unwrap();

        let iface = state.get_interface("eth0").unwrap();
        assert_eq!(iface.addresses.len(), 1); // Still there
    }

    #[test]
    fn test_interface_info_vpn_type() {
        let info = InterfaceInfo::new("tun0".to_string(), InterfaceType::Vpn);
        assert_eq!(info.interface_type, InterfaceType::Vpn);
    }

    #[test]
    fn test_interface_info_empty_addresses_no_ipv4_no_ipv6() {
        let info = InterfaceInfo::new("eth0".to_string(), InterfaceType::Lan);
        assert!(!info.has_ipv4());
        assert!(!info.has_ipv6());
        assert!(info.ipv4_addresses().is_empty());
        assert!(info.ipv6_addresses().is_empty());
    }

    #[test]
    fn test_interface_info_with_both_ipv4_and_ipv6() {
        let mut info = InterfaceInfo::new("eth0".to_string(), InterfaceType::Lan);
        info.addresses.push("192.168.1.1/24".parse().unwrap());
        info.addresses.push("2001:db8::1/64".parse().unwrap());
        assert!(info.has_ipv4());
        assert!(info.has_ipv6());
        assert_eq!(info.ipv4_addresses().len(), 1);
        assert_eq!(info.ipv6_addresses().len(), 1);
    }
}
