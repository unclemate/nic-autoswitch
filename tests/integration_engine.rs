//! Integration tests for the engine module
//!
//! These tests verify the event dispatcher and rule matching logic.

use std::collections::HashMap;
use std::sync::Arc;

use nic_autoswitch::config::{
    Config, GlobalConfig, InterfaceConfig, InterfaceType, MatchBy, RouteRule, RouteVia,
    RoutingConfig, WifiProfile,
};
use nic_autoswitch::engine::{DispatcherState, EventDispatcher};
use nic_autoswitch::monitor::{InterfaceChange, NetworkEvent};
use nic_autoswitch::router::RouteManager;

fn create_test_config() -> Config {
    let mut interfaces = HashMap::new();
    interfaces.insert(
        "eth0".to_string(),
        InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Name {
                name: "eth0".to_string(),
            },
            priority: 10,
        },
    );
    interfaces.insert(
        "wlan0".to_string(),
        InterfaceConfig {
            interface_type: InterfaceType::Wlan,
            match_by: MatchBy::Name {
                name: "wlan0".to_string(),
            },
            priority: 20,
        },
    );

    Config {
        global: GlobalConfig::default(),
        interfaces,
        wifi_profiles: HashMap::new(),
        routing: RoutingConfig::default(),
    }
}

fn create_wifi_profile_config() -> Config {
    let mut config = create_test_config();

    let profile = WifiProfile {
        interface: "wlan0".to_string(),
        rules: vec![RouteRule {
            name: "corp-cidr".to_string(),
            match_on: nic_autoswitch::config::MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        }],
    };

    config.wifi_profiles.insert("CorpWiFi".to_string(), profile);
    config
}

#[tokio::test]
async fn test_dispatcher_initialization() {
    let config = create_test_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    assert_eq!(dispatcher.state(), DispatcherState::Initializing);
}

#[tokio::test]
async fn test_dispatcher_start_stop() {
    let config = create_test_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    dispatcher.start();
    assert_eq!(dispatcher.state(), DispatcherState::Running);

    dispatcher.stop();
    assert_eq!(dispatcher.state(), DispatcherState::Stopped);
}

#[tokio::test]
async fn test_dispatcher_ignores_events_when_not_running() {
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

#[tokio::test]
async fn test_dispatcher_handles_wifi_connected() {
    let config = create_wifi_profile_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    dispatcher.start();

    let event = NetworkEvent::WifiConnected {
        interface: "wlan0".to_string(),
        ssid: "CorpWiFi".to_string(),
    };

    let result = dispatcher.handle_event(&event).await;
    assert!(result.is_ok());

    // Verify SSID is tracked
    assert_eq!(
        dispatcher.current_ssid("wlan0"),
        Some("CorpWiFi".to_string())
    );
}

#[tokio::test]
async fn test_dispatcher_handles_wifi_disconnected() {
    let config = create_wifi_profile_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    dispatcher.start();

    // First connect
    let connect_event = NetworkEvent::WifiConnected {
        interface: "wlan0".to_string(),
        ssid: "CorpWiFi".to_string(),
    };
    dispatcher.handle_event(&connect_event).await.unwrap();
    assert_eq!(
        dispatcher.current_ssid("wlan0"),
        Some("CorpWiFi".to_string())
    );

    // Then disconnect
    let disconnect_event = NetworkEvent::WifiDisconnected {
        interface: "wlan0".to_string(),
        last_ssid: Some("CorpWiFi".to_string()),
    };
    dispatcher.handle_event(&disconnect_event).await.unwrap();
    assert_eq!(dispatcher.current_ssid("wlan0"), None);
}

#[tokio::test]
async fn test_dispatcher_unknown_ssid_uses_default() {
    let config = create_wifi_profile_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    dispatcher.start();

    // Connect to an unknown SSID
    let event = NetworkEvent::WifiConnected {
        interface: "wlan0".to_string(),
        ssid: "UnknownWiFi".to_string(),
    };

    let result = dispatcher.handle_event(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_dispatcher_config_update() {
    let config = create_test_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    let mut new_config = create_test_config();
    new_config.global.log_level = "debug".to_string();

    dispatcher.update_config(new_config);
}

#[tokio::test]
async fn test_dispatcher_active_routes() {
    let config = create_test_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    // Initially no active routes
    let routes = dispatcher.active_routes();
    assert!(routes.is_empty());
}

#[tokio::test]
async fn test_dispatcher_network_state() {
    let config = create_test_config();
    let manager = Arc::new(RouteManager::default());
    let dispatcher = EventDispatcher::new(config, manager);

    let state = dispatcher.network_state();
    assert!(state.interfaces().is_empty());
}
