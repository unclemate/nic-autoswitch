//! Route rule executor
//!
//! This module provides rule execution logic for applying and removing routes.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;
use tracing::{debug, info, warn};

use crate::config::{RouteRule, WifiProfile};
use crate::router::{RouteManager, RuleOperator};

/// Active route entry for tracking
#[derive(Debug, Clone, Serialize)]
pub struct ActiveRoute {
    /// Rule name
    pub rule_name: String,
    /// Interface used
    pub interface: String,
    /// Table ID
    pub table_id: u32,
}

/// Rule executor for applying route changes
#[derive(Debug)]
pub struct RuleExecutor {
    /// Route manager (RwLock for late init: created with stub, replaced in async init)
    ///
    /// Safety: `parking_lot::RwLock` is synchronous and never held across `.await` points.
    /// All async methods clone the `Arc` before awaiting.
    route_manager: RwLock<Arc<RouteManager>>,
    /// Active routes tracking
    active_routes: RwLock<HashMap<String, ActiveRoute>>,
}

impl RuleExecutor {
    /// Create a new rule executor
    pub fn new(route_manager: Arc<RouteManager>) -> Self {
        Self {
            route_manager: RwLock::new(route_manager),
            active_routes: RwLock::new(HashMap::new()),
        }
    }

    /// Get a reference to the route manager
    pub fn route_manager(&self) -> parking_lot::RwLockReadGuard<'_, Arc<RouteManager>> {
        self.route_manager.read()
    }

    /// Replace the route manager (used during async init)
    pub fn set_route_manager(&self, rm: Arc<RouteManager>) {
        *self.route_manager.write() = rm;
    }

    /// Apply rules for a WiFi profile
    pub async fn apply_wifi_profile(
        &self,
        profile: &WifiProfile,
        interface_index: u8,
        table_id: u32,
    ) -> crate::Result<usize> {
        let mut applied_count = 0;

        for rule in &profile.rules {
            match self.apply_rule(rule, interface_index, table_id).await {
                Ok(()) => {
                    applied_count += 1;
                }
                Err(e) => {
                    warn!("Failed to apply rule '{}': {}", rule.name, e);
                }
            }
        }

        info!(
            "Applied {} rules for WiFi profile on interface {} (table {})",
            applied_count, profile.interface, table_id
        );

        Ok(applied_count)
    }

    /// Apply a single rule
    pub async fn apply_rule(
        &self,
        rule: &RouteRule,
        interface_index: u8,
        table_id: u32,
    ) -> crate::Result<()> {
        debug!("Applying rule: {} (table {})", rule.name, table_id);

        let operator = RuleOperator::new(self.route_manager.read().clone());

        // Validate rule first
        operator.validate_rule(rule)?;

        // Apply the rule
        operator.apply_rule(rule, interface_index, table_id).await?;

        // Track the active route
        self.track_active_route(&rule.name, &rule.route_via.interface, table_id);

        Ok(())
    }

    /// Remove a rule
    pub async fn remove_rule(&self, rule: &RouteRule, table_id: u32) -> crate::Result<()> {
        debug!("Removing rule: {} (table {})", rule.name, table_id);

        let operator = RuleOperator::new(self.route_manager.read().clone());
        operator.remove_rule(rule, table_id).await?;

        // Remove from tracking
        self.untrack_active_route(&rule.name);

        Ok(())
    }

    /// Flush all routes for an interface
    pub async fn flush_interface(&self, interface: &str, table_id: u32) -> crate::Result<usize> {
        info!(
            "Flushing routes for interface {} (table {})",
            interface, table_id
        );

        let rm = self.route_manager.read().clone();
        let count = rm.flush_interface_routes(interface, table_id).await?;

        // Remove tracking entries for this interface
        self.active_routes
            .write()
            .retain(|_, route| route.interface != interface);

        Ok(count)
    }

    /// Get active routes
    pub fn active_routes(&self) -> HashMap<String, ActiveRoute> {
        self.active_routes.read().clone()
    }

    /// Get active route count
    pub fn active_route_count(&self) -> usize {
        self.active_routes.read().len()
    }

    /// Get table ID for IPv4
    pub fn table_id_v4(&self, index: u8) -> u32 {
        self.route_manager.read().table_id_v4(index)
    }

    /// Get table ID for IPv6
    pub fn table_id_v6(&self, index: u8) -> u32 {
        self.route_manager.read().table_id_v6(index)
    }

    /// Check if a route is active
    pub fn is_route_active(&self, rule_name: &str) -> bool {
        self.active_routes.read().contains_key(rule_name)
    }

    /// Track an active route
    fn track_active_route(&self, rule_name: &str, interface: &str, table_id: u32) {
        let route = ActiveRoute {
            rule_name: rule_name.to_string(),
            interface: interface.to_string(),
            table_id,
        };
        self.active_routes
            .write()
            .insert(rule_name.to_string(), route);
        debug!("Tracking active route: {}", rule_name);
    }

    /// Untrack an active route
    fn untrack_active_route(&self, rule_name: &str) {
        self.active_routes.write().remove(rule_name);
        debug!("Untracked route: {}", rule_name);
    }

    /// Clear all active routes tracking
    pub fn clear_active_routes(&self) {
        self.active_routes.write().clear();
        debug!("Cleared all active route tracking");
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_rule(name: &str) -> RouteRule {
        RouteRule {
            name: name.to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: crate::config::RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        }
    }

    fn create_test_wifi_profile() -> WifiProfile {
        WifiProfile {
            interface: "wlan0".to_string(),
            rules: vec![create_test_rule("rule1"), create_test_rule("rule2")],
        }
    }

    #[test]
    fn test_rule_executor_new() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);
        assert_eq!(executor.active_route_count(), 0);
    }

    #[test]
    fn test_active_routes_returns_copy() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        let routes1 = executor.active_routes();
        let routes2 = executor.active_routes();
        assert_eq!(routes1.len(), routes2.len());
    }

    #[tokio::test]
    async fn test_apply_rule_increases_active_count() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        let rule = create_test_rule("test-rule");
        let result = executor.apply_rule(&rule, 0, 100).await;

        assert!(result.is_ok());
        assert_eq!(executor.active_route_count(), 1);
        assert!(executor.is_route_active("test-rule"));
    }

    #[tokio::test]
    async fn test_remove_rule_decreases_active_count() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        let rule = create_test_rule("test-rule");
        executor.apply_rule(&rule, 0, 100).await.unwrap();

        let result = executor.remove_rule(&rule, 100).await;
        assert!(result.is_ok());
        assert_eq!(executor.active_route_count(), 0);
        assert!(!executor.is_route_active("test-rule"));
    }

    #[tokio::test]
    async fn test_apply_wifi_profile_applies_all_rules() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        let profile = create_test_wifi_profile();
        let result = executor.apply_wifi_profile(&profile, 0, 100).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
        assert_eq!(executor.active_route_count(), 2);
    }

    #[tokio::test]
    async fn test_flush_interface_removes_routes() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        let rule = create_test_rule("test-rule");
        executor.apply_rule(&rule, 0, 100).await.unwrap();

        let result = executor.flush_interface("eth0", 100).await;
        assert!(result.is_ok());
        assert_eq!(executor.active_route_count(), 0);
    }

    #[test]
    fn test_clear_active_routes_removes_all() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        // Manually add routes
        executor.track_active_route("rule1", "eth0", 100);
        executor.track_active_route("rule2", "eth0", 100);

        assert_eq!(executor.active_route_count(), 2);

        executor.clear_active_routes();

        assert_eq!(executor.active_route_count(), 0);
    }

    #[test]
    fn test_is_route_active_returns_correct_status() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        assert!(!executor.is_route_active("nonexistent"));

        executor.track_active_route("test-rule", "eth0", 100);

        assert!(executor.is_route_active("test-rule"));
        assert!(!executor.is_route_active("other-rule"));
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_table_id_v4_delegates_to_route_manager() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);
        let table_id = executor.table_id_v4(5);
        // RouteManager::default() has table_id_start = 100
        // table_id_v4(5) = 100 + 5 = 105
        assert_eq!(table_id, 105);
    }

    #[test]
    fn test_table_id_v6_delegates_to_route_manager() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);
        let table_id = executor.table_id_v6(5);
        // RouteManager::default() has table_id_start = 100
        // table_id_v6(5) = 200 + 5 = 205 (IPv6 offset is +100)
        assert_eq!(table_id, 205);
    }

    #[tokio::test]
    async fn test_apply_wifi_profile_with_failing_rule_continues() {
        let manager = Arc::new(RouteManager::default());
        let executor = RuleExecutor::new(manager);

        // Create a profile with one valid rule and one invalid rule (domain-based)
        let valid_rule = RouteRule {
            name: "valid-rule".to_string(),
            match_on: crate::config::MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: crate::config::RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };

        let invalid_rule = RouteRule {
            name: "invalid-rule".to_string(),
            match_on: crate::config::MatchOn::Domain {
                domain: "example.com".to_string(),
            },
            route_via: crate::config::RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };

        let profile = WifiProfile {
            interface: "wlan0".to_string(),
            rules: vec![valid_rule, invalid_rule],
        };

        // Apply the profile - should succeed with partial results
        let result = executor.apply_wifi_profile(&profile, 0, 100).await;
        assert!(result.is_ok());
        // Only one rule should be applied (the valid CIDR one)
        assert_eq!(result.unwrap(), 1);
        assert_eq!(executor.active_route_count(), 1);
    }
}
