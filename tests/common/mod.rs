//! Shared test utilities for integration tests.

#[allow(dead_code)]
pub mod mock_network;

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use nic_autoswitch::config::{
    Config, GlobalConfig, InterfaceConfig, InterfaceType, MatchBy, RouteRule, RouteVia,
    RoutingConfig, WifiProfile,
};

/// Create a temporary config file with the given content.
#[allow(dead_code)]
pub fn create_temp_config(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write config");
    file.flush().expect("Failed to flush");
    file
}

/// Load a fixture file from `tests/fixtures/`.
#[allow(dead_code)]
pub fn load_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to load fixture '{name}': {e}"))
}

/// Builder for creating test `Config` instances.
#[allow(dead_code)]
pub struct TestConfigBuilder {
    global: GlobalConfig,
    interfaces: HashMap<String, InterfaceConfig>,
    wifi_profiles: HashMap<String, WifiProfile>,
    routing: RoutingConfig,
}

#[allow(dead_code)]
impl TestConfigBuilder {
    pub fn new() -> Self {
        Self {
            global: GlobalConfig::default(),
            interfaces: HashMap::new(),
            wifi_profiles: HashMap::new(),
            routing: RoutingConfig::default(),
        }
    }

    pub fn global(mut self, global: GlobalConfig) -> Self {
        self.global = global;
        self
    }

    pub fn interface(mut self, name: &str, iface_type: InterfaceType, priority: u32) -> Self {
        self.interfaces.insert(
            name.to_string(),
            InterfaceConfig {
                interface_type: iface_type,
                match_by: MatchBy::Name {
                    name: name.to_string(),
                },
                priority,
            },
        );
        self
    }

    pub fn wifi_profile(mut self, ssid: &str, interface: &str, rules: Vec<RouteRule>) -> Self {
        self.wifi_profiles.insert(
            ssid.to_string(),
            WifiProfile {
                interface: interface.to_string(),
                rules,
            },
        );
        self
    }

    pub fn default_rule(mut self, rule: RouteRule) -> Self {
        self.routing.default_rules.push(rule);
        self
    }

    pub fn build(self) -> Config {
        Config {
            global: self.global,
            interfaces: self.interfaces,
            wifi_profiles: self.wifi_profiles,
            routing: self.routing,
        }
    }
}

impl Default for TestConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a standard two-interface test config (nic0=LAN, nic1=WLAN).
#[allow(dead_code)]
pub fn standard_test_config() -> Config {
    TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .build()
}

/// Create a test config with CorpWiFi profile.
#[allow(dead_code)]
pub fn corp_wifi_config() -> Config {
    TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .wifi_profile(
            "CorpWiFi",
            "nic1",
            vec![RouteRule {
                name: "corp-cidr".to_string(),
                match_on: nic_autoswitch::config::MatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                route_via: RouteVia {
                    interface: "nic0".to_string(),
                },
                priority: 100,
            }],
        )
        .build()
}

/// Create a test config with multiple WiFi profiles.
#[allow(dead_code)]
pub fn multi_profile_config() -> Config {
    use nic_autoswitch::config::MatchOn;

    TestConfigBuilder::new()
        .interface("nic0", InterfaceType::Lan, 10)
        .interface("nic1", InterfaceType::Wlan, 20)
        .wifi_profile(
            "CorpWiFi",
            "nic1",
            vec![
                RouteRule {
                    name: "corp-10".to_string(),
                    match_on: MatchOn::Cidr {
                        cidr: "10.0.0.0/8".parse().unwrap(),
                    },
                    route_via: RouteVia {
                        interface: "nic0".to_string(),
                    },
                    priority: 100,
                },
                RouteRule {
                    name: "corp-172".to_string(),
                    match_on: MatchOn::Cidr {
                        cidr: "172.16.0.0/12".parse().unwrap(),
                    },
                    route_via: RouteVia {
                        interface: "nic0".to_string(),
                    },
                    priority: 110,
                },
            ],
        )
        .wifi_profile(
            "HomeWiFi",
            "nic1",
            vec![RouteRule {
                name: "home-nas".to_string(),
                match_on: MatchOn::Cidr {
                    cidr: "192.168.1.0/24".parse().unwrap(),
                },
                route_via: RouteVia {
                    interface: "nic1".to_string(),
                },
                priority: 100,
            }],
        )
        .default_rule(RouteRule {
            name: "default-ipv4".to_string(),
            match_on: MatchOn::Cidr {
                cidr: "0.0.0.0/0".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "nic1".to_string(),
            },
            priority: 10000,
        })
        .build()
}

/// Check if the test is running with CAP_NET_ADMIN capability.
/// Tests requiring network privileges should skip if this returns false.
#[allow(dead_code)]
pub fn has_net_admin() -> bool {
    // Try a simple network operation that requires CAP_NET_ADMIN
    std::process::Command::new("ip")
        .args(["link", "add", "test_cap_check", "type", "dummy"])
        .output()
        .map(|o| {
            // Clean up if it succeeded
            let _ = std::process::Command::new("ip")
                .args(["link", "del", "test_cap_check"])
                .output();
            o.status.success()
        })
        .unwrap_or(false)
}

/// Skip the test if not running with CAP_NET_ADMIN.
/// Call this at the beginning of tests that modify routing tables.
#[macro_export]
macro_rules! skip_without_net_admin {
    () => {
        if !$crate::common::has_net_admin() {
            eprintln!("SKIP: test requires CAP_NET_ADMIN (run in Docker)");
            return;
        }
    };
}
