//! Mock network components for integration testing.
//!
//! These mocks simulate network interfaces, events, and state without
//! requiring real hardware or CAP_NET_ADMIN.

use nic_autoswitch::monitor::{
    InterfaceChange, InterfaceInfo, InterfaceType, NetworkEvent, NetworkState,
};

/// Builder for creating mock `NetworkState` instances.
#[allow(dead_code)]
pub struct MockNetworkStateBuilder {
    interfaces: Vec<InterfaceInfo>,
}

impl MockNetworkStateBuilder {
    pub fn new() -> Self {
        Self {
            interfaces: Vec::new(),
        }
    }

    /// Add a LAN interface (up, with IPv4 address).
    pub fn lan(mut self, name: &str, addr: &str) -> Self {
        self.interfaces.push(InterfaceInfo {
            name: name.to_string(),
            interface_type: InterfaceType::Lan,
            addresses: vec![addr.parse().expect("Invalid address format")],
            is_up: true,
            ssid: None,
        });
        self
    }

    /// Add a WLAN interface (up, with IPv4 address and optional SSID).
    pub fn wlan(mut self, name: &str, addr: &str, ssid: Option<&str>) -> Self {
        self.interfaces.push(InterfaceInfo {
            name: name.to_string(),
            interface_type: InterfaceType::Wlan,
            addresses: vec![addr.parse().expect("Invalid address format")],
            is_up: true,
            ssid: ssid.map(|s| s.to_string()),
        });
        self
    }

    /// Build the `NetworkState`.
    pub fn build(self) -> NetworkState {
        let mut state = NetworkState::new();
        for info in self.interfaces {
            state.update_interface(info);
        }
        state
    }
}

impl Default for MockNetworkStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to create common network events for testing.
#[allow(dead_code)]
pub struct MockEventBuilder;

impl MockEventBuilder {
    /// Interface came up.
    pub fn interface_up(name: &str) -> NetworkEvent {
        NetworkEvent::InterfaceChanged {
            interface: name.to_string(),
            change: InterfaceChange::Up,
        }
    }

    /// Interface went down.
    pub fn interface_down(name: &str) -> NetworkEvent {
        NetworkEvent::InterfaceChanged {
            interface: name.to_string(),
            change: InterfaceChange::Down,
        }
    }

    /// Interface was added.
    pub fn interface_added(name: &str) -> NetworkEvent {
        NetworkEvent::InterfaceChanged {
            interface: name.to_string(),
            change: InterfaceChange::Added,
        }
    }

    /// Interface was removed.
    pub fn interface_removed(name: &str) -> NetworkEvent {
        NetworkEvent::InterfaceChanged {
            interface: name.to_string(),
            change: InterfaceChange::Removed,
        }
    }

    /// WiFi connected to an SSID.
    pub fn wifi_connected(interface: &str, ssid: &str) -> NetworkEvent {
        NetworkEvent::WifiConnected {
            interface: interface.to_string(),
            ssid: ssid.to_string(),
        }
    }

    /// WiFi disconnected from an SSID.
    pub fn wifi_disconnected(interface: &str, last_ssid: Option<&str>) -> NetworkEvent {
        NetworkEvent::WifiDisconnected {
            interface: interface.to_string(),
            last_ssid: last_ssid.map(|s| s.to_string()),
        }
    }
}

/// Standard mock state: nic0=LAN(192.168.1.100), nic1=WLAN(192.168.2.100).
#[allow(dead_code)]
pub fn standard_mock_state() -> NetworkState {
    MockNetworkStateBuilder::new()
        .lan("nic0", "192.168.1.100/24")
        .wlan("nic1", "192.168.2.100/24", None)
        .build()
}

/// Mock state with CorpWiFi connected.
#[allow(dead_code)]
pub fn corp_wifi_connected_state() -> NetworkState {
    MockNetworkStateBuilder::new()
        .lan("nic0", "192.168.1.100/24")
        .wlan("nic1", "192.168.2.100/24", Some("CorpWiFi"))
        .build()
}
