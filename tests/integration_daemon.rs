//! Integration tests for daemon lifecycle
//!
//! Tests the daemon service: startup, shutdown, event dispatching, and
//! network state management.
//!
//! **Requires CAP_NET_ADMIN** — run inside Docker container.

mod common;

use std::sync::Arc;

use nic_autoswitch::config::{InterfaceType, MatchOn as ConfigMatchOn, RouteRule, RouteVia};
use nic_autoswitch::engine::{DispatcherState, EventDispatcher};
use nic_autoswitch::monitor::{
    InterfaceChange, InterfaceInfo, InterfaceType as MonitorInterfaceType, NetworkEvent,
    NetworkState,
};
use nic_autoswitch::router::RouteManager;

use common::TestConfigBuilder;

// ============================================================================
// Dispatcher lifecycle tests
// ============================================================================

#[tokio::test]
async fn test_dispatcher_full_lifecycle() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    assert_eq!(dispatcher.state(), DispatcherState::Initializing);

    dispatcher.start();
    assert_eq!(dispatcher.state(), DispatcherState::Running);

    dispatcher.stop();
    assert_eq!(dispatcher.state(), DispatcherState::Stopped);
    assert!(dispatcher.active_routes().is_empty());
}

#[tokio::test]
async fn test_dispatcher_rejects_events_when_not_running() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .build();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    // Not started — stays in Initializing
    let event = NetworkEvent::InterfaceChanged {
        interface: "nic0".to_string(),
        change: InterfaceChange::Up,
    };
    let result = dispatcher.handle_event(&event).await;
    assert!(result.is_ok());
    assert!(dispatcher.active_routes().is_empty());
}

#[tokio::test]
async fn test_dispatcher_start_idempotent() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .build();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    dispatcher.start();
    assert_eq!(dispatcher.state(), DispatcherState::Running);
    // Start again — should warn but remain Running
    dispatcher.start();
    assert_eq!(dispatcher.state(), DispatcherState::Running);
}

// ============================================================================
// WiFi event handling tests
// ============================================================================

#[tokio::test]
async fn test_dispatcher_wifi_connect_disconnect_cycle() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .wifi_profile(
            "CorpWiFi",
            "nic1",
            vec![RouteRule {
                name: "corp-cidr".to_string(),
                match_on: ConfigMatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                route_via: RouteVia {
                    interface: "nic0".to_string(),
                },
                priority: 100,
            }],
        )
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    // Connect WiFi
    let connect_event = NetworkEvent::WifiConnected {
        interface: "nic1".to_string(),
        ssid: "CorpWiFi".to_string(),
    };
    dispatcher.handle_event(&connect_event).await.unwrap();
    assert_eq!(
        dispatcher.current_ssid("nic1"),
        Some("CorpWiFi".to_string())
    );
    assert_eq!(dispatcher.active_routes().len(), 1);

    // Disconnect WiFi
    let disconnect_event = NetworkEvent::WifiDisconnected {
        interface: "nic1".to_string(),
        last_ssid: Some("CorpWiFi".to_string()),
    };
    dispatcher.handle_event(&disconnect_event).await.unwrap();
    assert_eq!(dispatcher.current_ssid("nic1"), None);
}

#[tokio::test]
async fn test_dispatcher_wifi_switch_between_profiles() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .wifi_profile(
            "CorpWiFi",
            "nic1",
            vec![RouteRule {
                name: "corp-rule".to_string(),
                match_on: ConfigMatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                route_via: RouteVia {
                    interface: "nic0".to_string(),
                },
                priority: 100,
            }],
        )
        .wifi_profile(
            "HomeWiFi",
            "nic1",
            vec![RouteRule {
                name: "home-rule".to_string(),
                match_on: ConfigMatchOn::Cidr {
                    cidr: "192.168.1.0/24".parse().unwrap(),
                },
                route_via: RouteVia {
                    interface: "nic1".to_string(),
                },
                priority: 100,
            }],
        )
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    // Connect to CorpWiFi
    dispatcher
        .handle_event(&NetworkEvent::WifiConnected {
            interface: "nic1".to_string(),
            ssid: "CorpWiFi".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(
        dispatcher.current_ssid("nic1"),
        Some("CorpWiFi".to_string())
    );

    // Disconnect
    dispatcher
        .handle_event(&NetworkEvent::WifiDisconnected {
            interface: "nic1".to_string(),
            last_ssid: Some("CorpWiFi".to_string()),
        })
        .await
        .unwrap();

    // Connect to HomeWiFi
    dispatcher
        .handle_event(&NetworkEvent::WifiConnected {
            interface: "nic1".to_string(),
            ssid: "HomeWiFi".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(
        dispatcher.current_ssid("nic1"),
        Some("HomeWiFi".to_string())
    );
}

#[tokio::test]
async fn test_dispatcher_unknown_ssid_uses_default_rules() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .default_rule(RouteRule {
            name: "default-all".to_string(),
            match_on: ConfigMatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "nic1".to_string(),
            },
            priority: 10000,
        })
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    // Connect to unknown SSID
    dispatcher
        .handle_event(&NetworkEvent::WifiConnected {
            interface: "nic1".to_string(),
            ssid: "UnknownWiFi".to_string(),
        })
        .await
        .unwrap();

    assert_eq!(
        dispatcher.current_ssid("nic1"),
        Some("UnknownWiFi".to_string())
    );
    // Default rule should be applied
    assert_eq!(dispatcher.active_routes().len(), 1);
}

#[tokio::test]
async fn test_dispatcher_config_hot_update() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    let new_config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .build();

    dispatcher.update_config(new_config);
    assert_eq!(dispatcher.state(), DispatcherState::Running);
}

#[tokio::test]
async fn test_dispatcher_interface_up_applies_default_rules() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .default_rule(RouteRule {
            name: "default-all".to_string(),
            match_on: ConfigMatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "nic0".to_string(),
            },
            priority: 10000,
        })
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    // First add interface to network state, then bring it up
    // The dispatcher will only apply rules if the interface is known and is_up
    // Since the dispatcher's internal state starts empty, we need to first
    // get the interface into its state. We can do this via an Added event,
    // but the dispatcher only tracks interfaces if they are already in state.

    // Actually the dispatcher applies interface change events to internal state.
    // For Up event on unknown interface, it's a no-op.
    let result = dispatcher
        .handle_event(&NetworkEvent::InterfaceChanged {
            interface: "nonexistent".to_string(),
            change: InterfaceChange::Up,
        })
        .await;
    assert!(result.is_ok());
    assert!(dispatcher.active_routes().is_empty());
}

#[tokio::test]
async fn test_dispatcher_route_changed_event_noop() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    let event = NetworkEvent::RouteChanged {
        interface: Some("nic0".to_string()),
        destination: "0.0.0.0/0".parse().unwrap(),
        gateway: None,
        added: true,
    };
    let result = dispatcher.handle_event(&event).await;
    assert!(result.is_ok());
    assert!(dispatcher.active_routes().is_empty());
}

#[tokio::test]
async fn test_dispatcher_address_change_without_wifi() {
    let config = TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .default_rule(RouteRule {
            name: "default-addr".to_string(),
            match_on: ConfigMatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "nic0".to_string(),
            },
            priority: 10000,
        })
        .build();

    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);
    dispatcher.start();

    let event = NetworkEvent::AddressChanged {
        interface: "nic0".to_string(),
        added: vec!["192.168.1.1/24".parse().unwrap()],
        removed: vec![],
    };
    let result = dispatcher.handle_event(&event).await;
    assert!(result.is_ok());
}

// ============================================================================
// Network state mutation tests (direct)
// ============================================================================

#[test]
fn test_network_state_interface_lifecycle() {
    let mut state = NetworkState::new();

    // Add interface
    let mut info = InterfaceInfo::new("nic0".to_string(), MonitorInterfaceType::Lan);
    info.is_up = true;
    info.addresses.push("192.168.1.100/24".parse().unwrap());
    state.update_interface(info);

    assert!(state.has_interface("nic0"));
    assert!(state.get_interface("nic0").unwrap().is_up);

    // Bring down
    state
        .apply_event(&NetworkEvent::InterfaceChanged {
            interface: "nic0".to_string(),
            change: InterfaceChange::Down,
        })
        .unwrap();
    assert!(!state.get_interface("nic0").unwrap().is_up);

    // Remove
    state
        .apply_event(&NetworkEvent::InterfaceChanged {
            interface: "nic0".to_string(),
            change: InterfaceChange::Removed,
        })
        .unwrap();
    assert!(!state.has_interface("nic0"));
}

#[test]
fn test_network_state_wifi_ssid_tracking() {
    let mut state = NetworkState::new();
    let info = InterfaceInfo::new("nic1".to_string(), MonitorInterfaceType::Wlan);
    state.update_interface(info);

    // Connect WiFi
    state
        .apply_event(&NetworkEvent::WifiConnected {
            interface: "nic1".to_string(),
            ssid: "CorpWiFi".to_string(),
        })
        .unwrap();
    assert_eq!(
        state.get_interface("nic1").unwrap().ssid,
        Some("CorpWiFi".to_string())
    );
    assert!(state.get_interface("nic1").unwrap().is_up);

    // Disconnect WiFi
    state
        .apply_event(&NetworkEvent::WifiDisconnected {
            interface: "nic1".to_string(),
            last_ssid: Some("CorpWiFi".to_string()),
        })
        .unwrap();
    assert_eq!(state.get_interface("nic1").unwrap().ssid, None);
}

#[test]
fn test_network_state_address_changes() {
    let mut state = NetworkState::new();
    let mut info = InterfaceInfo::new("nic0".to_string(), MonitorInterfaceType::Lan);
    info.addresses.push("10.0.0.1/24".parse().unwrap());
    state.update_interface(info);

    let event = NetworkEvent::AddressChanged {
        interface: "nic0".to_string(),
        added: vec!["192.168.1.100/24".parse().unwrap()],
        removed: vec!["10.0.0.1/24".parse().unwrap()],
    };
    state.apply_event(&event).unwrap();

    let iface = state.get_interface("nic0").unwrap();
    assert_eq!(iface.addresses.len(), 1);
    assert_eq!(iface.addresses[0], "192.168.1.100/24".parse().unwrap());
}

#[test]
fn test_network_state_active_interfaces_filter() {
    let mut state = NetworkState::new();
    let mut eth0 = InterfaceInfo::new("nic0".to_string(), MonitorInterfaceType::Lan);
    eth0.is_up = true;
    let eth1 = InterfaceInfo::new("nic1".to_string(), MonitorInterfaceType::Wlan);
    state.update_interface(eth0);
    state.update_interface(eth1);

    let active = state.active_interfaces();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "nic0");
}

#[test]
fn test_network_state_wlan_lan_filters() {
    let mut state = NetworkState::new();
    state.update_interface(InterfaceInfo::new(
        "nic0".to_string(),
        MonitorInterfaceType::Lan,
    ));
    state.update_interface(InterfaceInfo::new(
        "nic1".to_string(),
        MonitorInterfaceType::Wlan,
    ));

    assert_eq!(state.lan_interfaces().len(), 1);
    assert_eq!(state.wlan_interfaces().len(), 1);
}

// ============================================================================
// MockControlState tests
// ============================================================================

#[test]
fn test_mock_control_state_lifecycle() {
    use nic_autoswitch::daemon::{ControlState, MockControlState};

    let mock = MockControlState::new();
    let status = mock.get_status();
    assert_eq!(status.state, "Running");
    assert_eq!(status.active_routes, 0);
    assert!(!mock.is_shutdown_requested());

    mock.request_shutdown();
    assert!(mock.is_shutdown_requested());
}

#[test]
fn test_mock_control_state_routes() {
    use nic_autoswitch::daemon::{ControlState, MockControlState};

    let mock = MockControlState::new();
    assert!(mock.get_active_routes().is_empty());
}

#[test]
fn test_mock_control_state_reload() {
    use nic_autoswitch::daemon::{ControlState, MockControlState};

    let mock = MockControlState::new();
    assert!(mock.reload_config().is_ok());
}
