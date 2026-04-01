//! Network event types
//!
//! This module defines all network events that the daemon monitors and responds to.

use ipnetwork::IpNetwork;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// Network event types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEvent {
    /// Interface state changed
    InterfaceChanged {
        /// Interface name
        interface: String,
        /// What changed
        change: InterfaceChange,
    },

    /// WiFi connected to a network
    WifiConnected {
        /// Interface name
        interface: String,
        /// SSID of connected WiFi
        ssid: String,
    },

    /// WiFi disconnected
    WifiDisconnected {
        /// Interface name
        interface: String,
        /// Last SSID before disconnect (if available)
        last_ssid: Option<String>,
    },

    /// IP address changed on interface
    AddressChanged {
        /// Interface name
        interface: String,
        /// Added addresses
        added: Vec<IpNetwork>,
        /// Removed addresses
        removed: Vec<IpNetwork>,
    },

    /// Route changed
    RouteChanged {
        /// Interface name (if applicable)
        interface: Option<String>,
        /// Destination network
        destination: IpNetwork,
        /// Gateway (if applicable)
        gateway: Option<IpAddr>,
        /// Whether route was added or removed
        added: bool,
    },
}

/// Interface state change types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterfaceChange {
    /// Interface came up
    Up,
    /// Interface went down
    Down,
    /// Interface was added
    Added,
    /// Interface was removed
    Removed,
}

impl NetworkEvent {
    /// Get the interface name associated with this event (if any)
    pub fn interface(&self) -> Option<&str> {
        match self {
            NetworkEvent::InterfaceChanged { interface, .. } => Some(interface),
            NetworkEvent::WifiConnected { interface, .. } => Some(interface),
            NetworkEvent::WifiDisconnected { interface, .. } => Some(interface),
            NetworkEvent::AddressChanged { interface, .. } => Some(interface),
            NetworkEvent::RouteChanged { interface, .. } => interface.as_deref(),
        }
    }

    /// Check if this event requires route recalculation
    pub fn requires_route_update(&self) -> bool {
        match self {
            NetworkEvent::InterfaceChanged { .. } => true,
            NetworkEvent::WifiConnected { .. } => true,
            NetworkEvent::WifiDisconnected { .. } => true,
            NetworkEvent::AddressChanged { .. } => true,
            NetworkEvent::RouteChanged { .. } => false, // Route already changed
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_ip_network() -> IpNetwork {
        "192.168.1.0/24".parse().unwrap()
    }

    // -------------------------------------------------------------------------
    // InterfaceChange Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_change_equality() {
        assert_eq!(InterfaceChange::Up, InterfaceChange::Up);
        assert_eq!(InterfaceChange::Down, InterfaceChange::Down);
        assert_ne!(InterfaceChange::Up, InterfaceChange::Down);
    }

    #[test]
    fn test_interface_change_copy() {
        let change = InterfaceChange::Up;
        let copy = change;
        assert_eq!(change, copy);
    }

    #[test]
    fn test_interface_change_debug_format() {
        assert!(format!("{:?}", InterfaceChange::Up).contains("Up"));
        assert!(format!("{:?}", InterfaceChange::Down).contains("Down"));
        assert!(format!("{:?}", InterfaceChange::Added).contains("Added"));
        assert!(format!("{:?}", InterfaceChange::Removed).contains("Removed"));
    }

    // -------------------------------------------------------------------------
    // NetworkEvent Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_changed_event_returns_interface() {
        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        assert_eq!(event.interface(), Some("eth0"));
    }

    #[test]
    fn test_wifi_connected_event_returns_interface() {
        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "MyWiFi".to_string(),
        };
        assert_eq!(event.interface(), Some("wlan0"));
    }

    #[test]
    fn test_wifi_disconnected_event_returns_interface() {
        let event = NetworkEvent::WifiDisconnected {
            interface: "wlan0".to_string(),
            last_ssid: Some("MyWiFi".to_string()),
        };
        assert_eq!(event.interface(), Some("wlan0"));
    }

    #[test]
    fn test_address_changed_event_returns_interface() {
        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec![create_test_ip_network()],
            removed: vec![],
        };
        assert_eq!(event.interface(), Some("eth0"));
    }

    #[test]
    fn test_route_changed_event_with_interface_returns_interface() {
        let event = NetworkEvent::RouteChanged {
            interface: Some("eth0".to_string()),
            destination: create_test_ip_network(),
            gateway: Some("192.168.1.1".parse().unwrap()),
            added: true,
        };
        assert_eq!(event.interface(), Some("eth0"));
    }

    #[test]
    fn test_route_changed_event_without_interface_returns_none() {
        let event = NetworkEvent::RouteChanged {
            interface: None,
            destination: create_test_ip_network(),
            gateway: None,
            added: true,
        };
        assert_eq!(event.interface(), None);
    }

    // -------------------------------------------------------------------------
    // requires_route_update Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_changed_requires_route_update() {
        let event = NetworkEvent::InterfaceChanged {
            interface: "eth0".to_string(),
            change: InterfaceChange::Up,
        };
        assert!(event.requires_route_update());
    }

    #[test]
    fn test_wifi_connected_requires_route_update() {
        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "MyWiFi".to_string(),
        };
        assert!(event.requires_route_update());
    }

    #[test]
    fn test_wifi_disconnected_requires_route_update() {
        let event = NetworkEvent::WifiDisconnected {
            interface: "wlan0".to_string(),
            last_ssid: None,
        };
        assert!(event.requires_route_update());
    }

    #[test]
    fn test_address_changed_requires_route_update() {
        let event = NetworkEvent::AddressChanged {
            interface: "eth0".to_string(),
            added: vec![],
            removed: vec![],
        };
        assert!(event.requires_route_update());
    }

    #[test]
    fn test_route_changed_does_not_require_route_update() {
        let event = NetworkEvent::RouteChanged {
            interface: Some("eth0".to_string()),
            destination: create_test_ip_network(),
            gateway: None,
            added: true,
        };
        assert!(!event.requires_route_update());
    }

    // -------------------------------------------------------------------------
    // Serialization Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_change_serialization_roundtrip() {
        let change = InterfaceChange::Up;
        let json = serde_json::to_string(&change).unwrap();
        let deserialized: InterfaceChange = serde_json::from_str(&json).unwrap();
        assert_eq!(change, deserialized);
    }

    #[test]
    fn test_network_event_serialization_roundtrip() {
        let event = NetworkEvent::WifiConnected {
            interface: "wlan0".to_string(),
            ssid: "MyWiFi".to_string(),
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: NetworkEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, deserialized);
    }
}
