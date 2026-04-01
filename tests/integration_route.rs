//! Integration tests for route management
//!
//! These tests verify route table operations using the kernel routing API.
//! **Requires CAP_NET_ADMIN** — run inside Docker container:
//!   docker compose run --rm test-integration

mod common;

use std::sync::Arc;

use nic_autoswitch::config::{MatchOn, RouteRule, RouteVia};
use nic_autoswitch::engine::RuleExecutor;
use nic_autoswitch::router::RouteManager;

/// Helper: create a CIDR route rule.
fn cidr_rule(name: &str, cidr: &str, interface: &str, priority: u32) -> RouteRule {
    RouteRule {
        name: name.to_string(),
        match_on: MatchOn::Cidr {
            cidr: cidr.parse().unwrap(),
        },
        route_via: RouteVia {
            interface: interface.to_string(),
        },
        priority,
    }
}

/// Helper: create an IP route rule.
fn ip_rule(name: &str, ip: &str, interface: &str, priority: u32) -> RouteRule {
    RouteRule {
        name: name.to_string(),
        match_on: MatchOn::Ip {
            ip: ip.parse().unwrap(),
        },
        route_via: RouteVia {
            interface: interface.to_string(),
        },
        priority,
    }
}

// ============================================================================
// RouteManager tests
// ============================================================================

#[test]
fn test_route_manager_new_valid_table_id() {
    let manager = RouteManager::new(100);
    assert!(manager.is_ok());
    assert_eq!(manager.unwrap().base_table_id(), 100);
}

#[test]
fn test_route_manager_new_boundary_table_ids() {
    // Lower boundary
    assert!(RouteManager::new(100).is_ok());
    assert!(RouteManager::new(99).is_err());

    // Upper boundary
    assert!(RouteManager::new(199).is_ok());
    assert!(RouteManager::new(200).is_err());
}

#[test]
fn test_route_manager_table_id_calculation() {
    let manager = RouteManager::new(100).unwrap();

    // IPv4: base + index
    assert_eq!(manager.table_id_v4(0), 100);
    assert_eq!(manager.table_id_v4(5), 105);

    // IPv6: 200 + index
    assert_eq!(manager.table_id_v6(0), 200);
    assert_eq!(manager.table_id_v6(5), 205);
}

#[tokio::test]
async fn test_add_and_remove_ipv4_route() {
    let manager = RouteManager::default();
    let dest = "10.0.0.0/8".parse().unwrap();

    // Add route
    let result = manager.add_route(dest, None, "nic0", 100, 100).await;
    assert!(result.is_ok());

    // Remove route
    let result = manager.remove_route(dest, 100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_add_route_with_gateway() {
    let manager = RouteManager::default();
    let dest = "192.168.1.0/24".parse().unwrap();
    let gateway: Option<std::net::IpAddr> = Some("192.168.1.1".parse().unwrap());

    let result = manager.add_route(dest, gateway, "nic0", 100, 100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_remove_nonexistent_route_idempotent() {
    let manager = RouteManager::default();
    let dest = "10.99.0.0/16".parse().unwrap();

    // Removing a route that was never added should succeed (idempotent)
    let result = manager.remove_route(dest, 100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_flush_interface_routes() {
    let manager = RouteManager::default();

    let result = manager.flush_interface_routes("nic0", 100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_add_and_remove_policy_rule() {
    let manager = RouteManager::default();
    let dest = "10.0.0.0/8".parse().unwrap();

    // Add policy rule
    let result = manager.add_policy_rule(Some(dest), None, 100, 100).await;
    assert!(result.is_ok());

    // Remove policy rule
    let result = manager.remove_policy_rule(Some(dest), None, 100, 100).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_route_exists_check() {
    let manager = RouteManager::default();
    let dest = "10.0.0.0/8".parse().unwrap();

    // Stub implementation always returns false
    let result = manager.route_exists(dest, 100).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

// ============================================================================
// RuleOperator tests (via RuleExecutor)
// ============================================================================

#[tokio::test]
async fn test_apply_cidr_rule_tracking() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = cidr_rule("corp-10", "10.0.0.0/8", "nic0", 100);
    let result = executor.apply_rule(&rule, 0, 100).await;

    assert!(result.is_ok());
    assert!(executor.is_route_active("corp-10"));
    assert_eq!(executor.active_route_count(), 1);
}

#[tokio::test]
async fn test_apply_and_remove_cidr_rule() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = cidr_rule("corp-10", "10.0.0.0/8", "nic0", 100);

    // Apply
    executor.apply_rule(&rule, 0, 100).await.unwrap();
    assert!(executor.is_route_active("corp-10"));

    // Remove
    executor.remove_rule(&rule, 100).await.unwrap();
    assert!(!executor.is_route_active("corp-10"));
}

#[tokio::test]
async fn test_apply_ip_rule_single_host() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = ip_rule("single-ip", "203.0.113.50", "nic0", 120);
    let result = executor.apply_rule(&rule, 0, 100).await;

    assert!(result.is_ok());
    assert!(executor.is_route_active("single-ip"));
}

#[tokio::test]
async fn test_apply_domain_rule_returns_error() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = RouteRule {
        name: "domain-rule".to_string(),
        match_on: MatchOn::Domain {
            domain: "gitlab.corp.com".to_string(),
        },
        route_via: RouteVia {
            interface: "nic0".to_string(),
        },
        priority: 130,
    };

    let result = executor.apply_rule(&rule, 0, 100).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("DNS resolution"));
}

#[tokio::test]
async fn test_apply_domain_pattern_rule_returns_error() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = RouteRule {
        name: "pattern-rule".to_string(),
        match_on: MatchOn::DomainPattern {
            domain_pattern: "*.corp.example.com".to_string(),
        },
        route_via: RouteVia {
            interface: "nic0".to_string(),
        },
        priority: 140,
    };

    let result = executor.apply_rule(&rule, 0, 100).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_apply_wifi_profile_multiple_rules() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    use nic_autoswitch::config::WifiProfile;

    let profile = WifiProfile {
        interface: "nic1".to_string(),
        rules: vec![
            cidr_rule("corp-10", "10.0.0.0/8", "nic0", 100),
            cidr_rule("corp-172", "172.16.0.0/12", "nic0", 110),
            ip_rule("corp-ip", "203.0.113.50", "nic0", 120),
        ],
    };

    let result = executor.apply_wifi_profile(&profile, 0, 100).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 3);
    assert_eq!(executor.active_route_count(), 3);
}

#[tokio::test]
async fn test_flush_interface_clears_tracking() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule = cidr_rule("test-rule", "10.0.0.0/8", "nic0", 100);
    executor.apply_rule(&rule, 0, 100).await.unwrap();
    assert_eq!(executor.active_route_count(), 1);

    let result = executor.flush_interface("nic0", 100).await;
    assert!(result.is_ok());
    assert_eq!(executor.active_route_count(), 0);
}

#[tokio::test]
async fn test_multiple_interfaces_isolated() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    let rule_nic0 = cidr_rule("nic0-rule", "10.0.0.0/8", "nic0", 100);
    let rule_nic1 = cidr_rule("nic1-rule", "172.16.0.0/12", "nic1", 100);

    executor.apply_rule(&rule_nic0, 0, 100).await.unwrap();
    executor.apply_rule(&rule_nic1, 1, 101).await.unwrap();

    assert_eq!(executor.active_route_count(), 2);

    // Flush nic0 only
    executor.flush_interface("nic0", 100).await.unwrap();
    assert_eq!(executor.active_route_count(), 1);
    assert!(!executor.is_route_active("nic0-rule"));
    assert!(executor.is_route_active("nic1-rule"));
}

#[tokio::test]
async fn test_clear_active_routes() {
    let manager = Arc::new(RouteManager::default());
    let executor = RuleExecutor::new(manager);

    executor
        .apply_rule(&cidr_rule("r1", "10.0.0.0/8", "nic0", 100), 0, 100)
        .await
        .unwrap();
    executor
        .apply_rule(&cidr_rule("r2", "172.16.0.0/12", "nic0", 110), 0, 100)
        .await
        .unwrap();

    assert_eq!(executor.active_route_count(), 2);
    executor.clear_active_routes();
    assert_eq!(executor.active_route_count(), 0);
}
