//! Integration tests for configuration loading
//!
//! These tests verify the configuration loading and validation logic.

use std::io::Write;
use tempfile::NamedTempFile;

use nic_autoswitch::config::ConfigLoader;

fn create_temp_config(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write config");
    file.flush().expect("Failed to flush");
    file
}

#[test]
fn test_load_valid_config() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
dry_run = false
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_ok());

    let loader = result.unwrap();
    let config = loader.current();
    assert_eq!(config.global.monitor_interval, 5);
    assert_eq!(config.interfaces.len(), 2);
}

#[test]
fn test_load_config_with_wifi_profile() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10

[interfaces.wlan0]
interface_type = "wlan"
match_by = { name = "wlan0" }
priority = 20

[wifi_profiles."CorpWiFi"]
interface = "wlan0"

[[wifi_profiles."CorpWiFi".rules]]
name = "corp-cidr"
match_on = { cidr = "10.0.0.0/8" }
route_via = { interface = "eth0" }
priority = 100
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_ok());

    let loader = result.unwrap();
    let config = loader.current();
    assert!(config.wifi_profiles.contains_key("CorpWiFi"));
    let profile = config.wifi_profiles.get("CorpWiFi").unwrap();
    assert_eq!(profile.rules.len(), 1);
}

#[test]
fn test_load_config_with_default_rules() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10

[[routing.default_rules]]
name = "default-wlan"
match_on = { cidr = "0.0.0.0/0" }
route_via = { interface = "eth0" }
priority = 10000
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_ok());

    let loader = result.unwrap();
    let config = loader.current();
    assert_eq!(config.routing.default_rules.len(), 1);
}

#[test]
fn test_load_config_missing_file() {
    let result = ConfigLoader::new("/nonexistent/path/config.toml");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn test_load_config_invalid_toml() {
    let content = "invalid [[[ toml syntax";
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Failed to parse"));
}

#[test]
fn test_load_config_empty_interfaces() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("At least one interface"));
}

#[test]
fn test_load_config_invalid_table_id() {
    let content = r#"
[global]
monitor_interval = 5
table_id_start = 50

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("table_id_start"));
}

#[test]
fn test_config_reload() {
    let content = r#"
[global]
monitor_interval = 5
log_level = "info"
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10
"#;
    let file = create_temp_config(content);
    let mut loader = ConfigLoader::new(file.path()).expect("Failed to load config");

    assert_eq!(loader.current().global.monitor_interval, 5);

    // Update the config file
    let updated_content = r#"
[global]
monitor_interval = 10
log_level = "debug"
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
priority = 10
"#;
    std::fs::write(file.path(), updated_content).expect("Failed to update config");

    // Reload
    let result = loader.reload();
    assert!(result.is_ok());
    assert_eq!(loader.current().global.monitor_interval, 10);
    assert_eq!(loader.current().global.log_level, "debug");
}

#[test]
fn test_config_validation_ip_match() {
    let content = r#"
[global]
monitor_interval = 5
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "test-ip"
match_on = { ip = "192.168.1.1" }
route_via = { interface = "eth0" }
priority = 100
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_ok());
}

#[test]
fn test_config_validation_domain_match() {
    let content = r#"
[global]
monitor_interval = 5
table_id_start = 100

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }

[[routing.default_rules]]
name = "test-domain"
match_on = { domain = "example.com" }
route_via = { interface = "eth0" }
priority = 100
"#;
    let file = create_temp_config(content);
    let result = ConfigLoader::new(file.path());
    assert!(result.is_ok());
}
