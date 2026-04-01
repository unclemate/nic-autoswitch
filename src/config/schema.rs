//! Configuration data structures
//!
//! This module defines all configuration types and their validation logic.

use ipnetwork::IpNetwork;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

/// Maximum table_id_start value (netlink u8 limits effective range to 252)
pub const MAX_TABLE_ID_START: u32 = 252;

/// Top-level configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Global settings
    #[serde(default)]
    pub global: GlobalConfig,

    /// Interface configurations
    #[serde(default)]
    pub interfaces: HashMap<String, InterfaceConfig>,

    /// WiFi profile-specific rules
    #[serde(default)]
    pub wifi_profiles: HashMap<String, WifiProfile>,

    /// Routing configuration
    #[serde(default)]
    pub routing: RoutingConfig,
}

impl Config {
    /// Validate the entire configuration
    pub fn validate(&self) -> crate::Result<()> {
        self.global.validate()?;

        if self.interfaces.is_empty() {
            return Err(crate::NicAutoSwitchError::Config(
                "At least one interface must be configured".to_string(),
            ));
        }

        for (name, iface) in &self.interfaces {
            iface.validate().map_err(|e| {
                crate::NicAutoSwitchError::Config(format!("interface '{}': {}", name, e))
            })?;
        }

        for (ssid, profile) in &self.wifi_profiles {
            profile.validate().map_err(|e| {
                crate::NicAutoSwitchError::Config(format!("wifi_profile '{}': {}", ssid, e))
            })?;
        }

        self.routing.validate()?;

        Ok(())
    }
}

/// Global configuration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Network state monitoring interval in seconds
    #[serde(default = "default_monitor_interval")]
    pub monitor_interval: u64,

    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// If true, don't actually modify routing tables
    #[serde(default)]
    pub dry_run: bool,

    /// Starting routing table ID for IPv4 (100-199)
    /// IPv6 tables use 200-299
    #[serde(default = "default_table_id_start")]
    pub table_id_start: u32,
}

fn default_monitor_interval() -> u64 {
    5
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_table_id_start() -> u32 {
    100
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            monitor_interval: default_monitor_interval(),
            log_level: default_log_level(),
            dry_run: false,
            table_id_start: default_table_id_start(),
        }
    }
}

impl GlobalConfig {
    /// Validate global configuration
    pub fn validate(&self) -> crate::Result<()> {
        if self.monitor_interval == 0 {
            return Err(crate::NicAutoSwitchError::Config(
                "monitor_interval must be greater than 0".to_string(),
            ));
        }

        // Validate log level
        const VALID_LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
        if !VALID_LOG_LEVELS.contains(&self.log_level.to_lowercase().as_str()) {
            return Err(crate::NicAutoSwitchError::Config(format!(
                "Invalid log_level '{}'. Must be one of: {:?}",
                self.log_level, VALID_LOG_LEVELS
            )));
        }

        // Validate table_id_start range
        if self.table_id_start < 100 || self.table_id_start > MAX_TABLE_ID_START {
            return Err(crate::NicAutoSwitchError::Config(format!(
                "table_id_start must be between 100 and {}",
                MAX_TABLE_ID_START
            )));
        }

        Ok(())
    }
}

/// Interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceType {
    /// Wired LAN interface
    Lan,
    /// Wireless WLAN interface
    Wlan,
    /// VPN interface
    Vpn,
}

/// Interface matching criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchBy {
    /// Match by exact interface name
    Name { name: String },
    /// Match by interface type (e.g., "eth*")
    Pattern { pattern: String },
    /// Match by MAC address
    Mac { mac: String },
}

/// Interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceConfig {
    /// Interface type
    pub interface_type: InterfaceType,

    /// How to match this interface
    pub match_by: MatchBy,

    /// Priority (lower = higher priority)
    #[serde(default = "default_interface_priority")]
    pub priority: u32,
}

fn default_interface_priority() -> u32 {
    100
}

impl InterfaceConfig {
    /// Validate interface configuration
    pub fn validate(&self) -> crate::Result<()> {
        // Validate match_by
        match &self.match_by {
            MatchBy::Name { name } if name.is_empty() => {
                return Err(crate::NicAutoSwitchError::Config(
                    "interface name cannot be empty".to_string(),
                ));
            }
            MatchBy::Pattern { pattern } if pattern.is_empty() => {
                return Err(crate::NicAutoSwitchError::Config(
                    "interface pattern cannot be empty".to_string(),
                ));
            }
            MatchBy::Mac { mac } if mac.is_empty() => {
                return Err(crate::NicAutoSwitchError::Config(
                    "MAC address cannot be empty".to_string(),
                ));
            }
            _ => {}
        }

        Ok(())
    }
}

/// Route matching condition
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MatchOn {
    /// Match CIDR network (e.g., "10.0.0.0/8", "2001:db8::/32")
    Cidr { cidr: IpNetwork },
    /// Match exact IP address
    Ip { ip: IpAddr },
    /// Match exact domain name
    Domain { domain: String },
    /// Match domain pattern (supports wildcard like "*.example.com")
    DomainPattern { domain_pattern: String },
}

/// Route target
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteVia {
    /// Target interface name
    pub interface: String,
}

/// Routing rule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteRule {
    /// Rule name for identification
    pub name: String,

    /// Matching condition
    pub match_on: MatchOn,

    /// Route target
    pub route_via: RouteVia,

    /// Priority (lower = higher priority)
    pub priority: u32,
}

impl RouteRule {
    /// Validate routing rule
    pub fn validate(&self) -> crate::Result<()> {
        if self.name.is_empty() {
            return Err(crate::NicAutoSwitchError::Config(
                "rule name cannot be empty".to_string(),
            ));
        }

        // Validate domain patterns
        if let MatchOn::DomainPattern { domain_pattern } = &self.match_on
            && !domain_pattern.starts_with('*')
        {
            return Err(crate::NicAutoSwitchError::Config(format!(
                "domain_pattern '{}' must start with '*' for wildcard matching",
                domain_pattern
            )));
        }

        Ok(())
    }
}

/// WiFi profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiProfile {
    /// Associated interface
    pub interface: String,

    /// Rules for this WiFi profile
    #[serde(default)]
    pub rules: Vec<RouteRule>,
}

impl WifiProfile {
    /// Validate WiFi profile
    pub fn validate(&self) -> crate::Result<()> {
        if self.interface.is_empty() {
            return Err(crate::NicAutoSwitchError::Config(
                "interface cannot be empty".to_string(),
            ));
        }

        for rule in &self.rules {
            rule.validate()?;
        }

        Ok(())
    }
}

/// Routing configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Default routing rules (applied when no WiFi profile matches)
    #[serde(default)]
    pub default_rules: Vec<RouteRule>,
}

impl RoutingConfig {
    /// Validate routing configuration
    pub fn validate(&self) -> crate::Result<()> {
        for rule in &self.default_rules {
            rule.validate()?;
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_valid_global_config() -> GlobalConfig {
        GlobalConfig {
            monitor_interval: 5,
            log_level: "info".to_string(),
            dry_run: false,
            table_id_start: 100,
        }
    }

    fn create_valid_interface_config() -> InterfaceConfig {
        InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Name {
                name: "eth0".to_string(),
            },
            priority: 10,
        }
    }

    fn create_valid_route_rule() -> RouteRule {
        RouteRule {
            name: "test-rule".to_string(),
            match_on: MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        }
    }

    // -------------------------------------------------------------------------
    // GlobalConfig Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_global_config_default_values_are_valid() {
        let config = GlobalConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_global_config_with_zero_interval_returns_error() {
        let mut config = create_valid_global_config();
        config.monitor_interval = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("monitor_interval"));
    }

    #[test]
    fn test_global_config_with_invalid_log_level_returns_error() {
        let mut config = create_valid_global_config();
        config.log_level = "invalid".to_string();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("log_level"));
    }

    #[test]
    fn test_global_config_with_table_id_below_range_returns_error() {
        let mut config = create_valid_global_config();
        config.table_id_start = 50;
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("table_id_start"));
    }

    #[test]
    fn test_global_config_with_table_id_above_range_returns_error() {
        let mut config = create_valid_global_config();
        config.table_id_start = 300;
        let result = config.validate();
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // InterfaceConfig Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_interface_config_with_valid_name_succeeds() {
        let config = create_valid_interface_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_interface_config_with_empty_name_returns_error() {
        let config = InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Name {
                name: "".to_string(),
            },
            priority: 10,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("name cannot be empty")
        );
    }

    #[test]
    fn test_interface_config_with_empty_pattern_returns_error() {
        let config = InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Pattern {
                pattern: "".to_string(),
            },
            priority: 10,
        };
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_interface_config_with_empty_mac_returns_error() {
        let config = InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Mac {
                mac: "".to_string(),
            },
            priority: 10,
        };
        let result = config.validate();
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // RouteRule Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_route_rule_with_valid_cidr_succeeds() {
        let rule = create_valid_route_rule();
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn test_route_rule_with_empty_name_returns_error() {
        let mut rule = create_valid_route_rule();
        rule.name = "".to_string();
        let result = rule.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("name cannot be empty")
        );
    }

    #[test]
    fn test_route_rule_with_valid_domain_pattern_succeeds() {
        let rule = RouteRule {
            name: "wildcard".to_string(),
            match_on: MatchOn::DomainPattern {
                domain_pattern: "*.example.com".to_string(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn test_route_rule_with_invalid_domain_pattern_returns_error() {
        let rule = RouteRule {
            name: "invalid-wildcard".to_string(),
            match_on: MatchOn::DomainPattern {
                domain_pattern: "example.com".to_string(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };
        let result = rule.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must start with '*'")
        );
    }

    // -------------------------------------------------------------------------
    // Config Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_config_with_empty_interfaces_returns_error() {
        let config = Config::default();
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("At least one interface")
        );
    }

    #[test]
    fn test_config_with_valid_interface_succeeds() {
        let mut config = Config::default();
        config
            .interfaces
            .insert("eth0".to_string(), create_valid_interface_config());
        assert!(config.validate().is_ok());
    }

    // -------------------------------------------------------------------------
    // MatchOn Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_match_on_cidr_parses_ipv4_correctly() {
        let match_on = MatchOn::Cidr {
            cidr: "10.0.0.0/8".parse().unwrap(),
        };
        if let MatchOn::Cidr { cidr } = match_on {
            assert!(cidr.contains("10.1.2.3".parse().unwrap()));
            assert!(!cidr.contains("8.8.8.8".parse().unwrap()));
        } else {
            panic!("Expected Cidr variant");
        }
    }

    #[test]
    fn test_match_on_cidr_parses_ipv6_correctly() {
        let match_on = MatchOn::Cidr {
            cidr: "2001:db8::/32".parse().unwrap(),
        };
        if let MatchOn::Cidr { cidr } = match_on {
            assert!(cidr.contains("2001:db8::1".parse().unwrap()));
            assert!(!cidr.contains("2001:db9::1".parse().unwrap()));
        } else {
            panic!("Expected Cidr variant");
        }
    }

    #[test]
    fn test_match_on_ip_accepts_valid_ipv4() {
        let match_on = MatchOn::Ip {
            ip: "192.168.1.1".parse().unwrap(),
        };
        if let MatchOn::Ip { ip } = match_on {
            assert!(ip.is_ipv4());
        } else {
            panic!("Expected Ip variant");
        }
    }

    #[test]
    fn test_match_on_ip_accepts_valid_ipv6() {
        let match_on = MatchOn::Ip {
            ip: "::1".parse().unwrap(),
        };
        if let MatchOn::Ip { ip } = match_on {
            assert!(ip.is_ipv6());
        } else {
            panic!("Expected Ip variant");
        }
    }

    #[test]
    fn test_wifi_profile_with_empty_interface_returns_error() {
        let profile = WifiProfile {
            interface: "".to_string(),
            rules: vec![create_valid_route_rule()],
        };
        let result = profile.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("interface cannot be empty")
        );
    }

    #[test]
    fn test_config_with_wifi_profile_and_default_rules_succeeds() {
        let mut config = Config::default();
        config
            .interfaces
            .insert("eth0".to_string(), create_valid_interface_config());
        config.wifi_profiles.insert(
            "CorpWiFi".to_string(),
            WifiProfile {
                interface: "wlan0".to_string(),
                rules: vec![create_valid_route_rule()],
            },
        );
        config.routing.default_rules.push(create_valid_route_rule());
        assert!(config.validate().is_ok());
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_global_config_case_insensitive_log_level() {
        let mut config = create_valid_global_config();
        config.log_level = "INFO".to_string();
        assert!(config.validate().is_ok());

        config.log_level = "Debug".to_string();
        assert!(config.validate().is_ok());

        config.log_level = "WARN".to_string();
        assert!(config.validate().is_ok());

        config.log_level = "ERROR".to_string();
        assert!(config.validate().is_ok());

        config.log_level = "TRACE".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_global_config_table_id_boundary_values() {
        // Exact lower bound
        let mut config = create_valid_global_config();
        config.table_id_start = 100;
        assert!(config.validate().is_ok());

        // Exact upper bound
        config.table_id_start = MAX_TABLE_ID_START;
        assert!(config.validate().is_ok());

        // One below lower bound
        config.table_id_start = 99;
        assert!(config.validate().is_err());

        // One above upper bound
        config.table_id_start = MAX_TABLE_ID_START + 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_toml_deserialization_full() {
        let toml = r#"
[global]
monitor_interval = 10
log_level = "debug"
dry_run = true
table_id_start = 150

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10

[interfaces.wlan0]
interface_type = "wlan"
match_by = { pattern = "wlan*" }
priority = 20
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.global.monitor_interval, 10);
        assert_eq!(config.global.log_level, "debug");
        assert!(config.global.dry_run);
        assert_eq!(config.global.table_id_start, 150);
        assert_eq!(config.interfaces.len(), 2);
    }

    #[test]
    fn test_config_toml_deserialization_minimal() {
        let toml = r#"
[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
        let config: Config = toml::from_str(toml).unwrap();
        // Defaults
        assert_eq!(config.global.monitor_interval, 5);
        assert_eq!(config.global.log_level, "info");
        assert!(!config.global.dry_run);
        assert_eq!(config.global.table_id_start, 100);
        assert!(config.wifi_profiles.is_empty());
        assert!(config.routing.default_rules.is_empty());
    }

    #[test]
    fn test_config_toml_mac_match() {
        let toml = r#"
[interfaces.eth0]
interface_type = "lan"
match_by = { mac = "aa:bb:cc:dd:ee:ff" }
priority = 5
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.interfaces.contains_key("eth0"));
        if let MatchBy::Mac { mac } = &config.interfaces["eth0"].match_by {
            assert_eq!(mac, "aa:bb:cc:dd:ee:ff");
        } else {
            panic!("Expected Mac variant");
        }
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let mut config = Config::default();
        config
            .interfaces
            .insert("eth0".to_string(), create_valid_interface_config());
        config.routing.default_rules.push(create_valid_route_rule());

        let toml_str = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(
            config.global.monitor_interval,
            deserialized.global.monitor_interval
        );
        assert_eq!(config.interfaces.len(), deserialized.interfaces.len());
        assert_eq!(
            config.routing.default_rules.len(),
            deserialized.routing.default_rules.len()
        );
    }

    #[test]
    fn test_match_on_ip_serialization() {
        let toml = r#"
[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "exact-ip"
match_on = { ip = "192.168.1.1" }
route_via = { interface = "eth0" }
priority = 100
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.routing.default_rules.len(), 1);
        assert!(matches!(
            config.routing.default_rules[0].match_on,
            MatchOn::Ip { .. }
        ));
    }

    #[test]
    fn test_match_on_domain_serialization() {
        let toml = r#"
[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "domain-rule"
match_on = { domain = "example.com" }
route_via = { interface = "eth0" }
priority = 100
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(matches!(
            config.routing.default_rules[0].match_on,
            MatchOn::Domain { .. }
        ));
    }

    #[test]
    fn test_match_on_domain_pattern_serialization() {
        let toml = r#"
[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "pattern-rule"
match_on = { domain_pattern = "*.example.com" }
route_via = { interface = "eth0" }
priority = 100
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(matches!(
            config.routing.default_rules[0].match_on,
            MatchOn::DomainPattern { .. }
        ));
    }

    #[test]
    fn test_interface_config_with_valid_pattern_succeeds() {
        let config = InterfaceConfig {
            interface_type: InterfaceType::Wlan,
            match_by: MatchBy::Pattern {
                pattern: "wlan*".to_string(),
            },
            priority: 20,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_interface_config_with_valid_mac_succeeds() {
        let config = InterfaceConfig {
            interface_type: InterfaceType::Lan,
            match_by: MatchBy::Mac {
                mac: "aa:bb:cc:dd:ee:ff".to_string(),
            },
            priority: 10,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_wifi_profile_with_valid_interface_and_no_rules_succeeds() {
        let profile = WifiProfile {
            interface: "wlan0".to_string(),
            rules: vec![],
        };
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn test_config_with_invalid_interface_in_interfaces_returns_error() {
        let mut config = Config::default();
        config.interfaces.insert(
            "bad".to_string(),
            InterfaceConfig {
                interface_type: InterfaceType::Lan,
                match_by: MatchBy::Name {
                    name: "".to_string(),
                },
                priority: 10,
            },
        );
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("interface 'bad'"));
    }

    #[test]
    fn test_config_with_invalid_wifi_profile_returns_error() {
        let mut config = Config::default();
        config
            .interfaces
            .insert("eth0".to_string(), create_valid_interface_config());
        config.wifi_profiles.insert(
            "BadProfile".to_string(),
            WifiProfile {
                interface: "".to_string(),
                rules: vec![],
            },
        );
        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("wifi_profile 'BadProfile'")
        );
    }

    #[test]
    fn test_config_with_invalid_default_rule_returns_error() {
        let mut config = Config::default();
        config
            .interfaces
            .insert("eth0".to_string(), create_valid_interface_config());
        config.routing.default_rules.push(RouteRule {
            name: "".to_string(),
            match_on: MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        });
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_route_rule_with_valid_domain_succeeds() {
        let rule = RouteRule {
            name: "domain-rule".to_string(),
            match_on: MatchOn::Domain {
                domain: "example.com".to_string(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };
        assert!(rule.validate().is_ok());
    }

    #[test]
    fn test_route_via_equality() {
        let via1 = RouteVia {
            interface: "eth0".to_string(),
        };
        let via2 = RouteVia {
            interface: "eth0".to_string(),
        };
        assert_eq!(via1, via2);
    }

    #[test]
    fn test_interface_type_serialization() {
        let toml = r#"
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10
"#;
        let config: InterfaceConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.interface_type, InterfaceType::Lan);

        let toml = r#"
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20
"#;
        let config: InterfaceConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.interface_type, InterfaceType::Wlan);

        let toml = r#"
interface_type = "vpn"
match_by = { name = "tun0" }
priority = 30
"#;
        let config: InterfaceConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.interface_type, InterfaceType::Vpn);
    }

    #[test]
    fn test_global_config_debug_format() {
        let config = GlobalConfig::default();
        let debug = format!("{:?}", config);
        assert!(debug.contains("monitor_interval"));
        assert!(debug.contains("log_level"));
    }
}
