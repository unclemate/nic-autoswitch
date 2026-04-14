//! Route rule operations
//!
//! This module provides route rule application and removal.

use ipnetwork::IpNetwork;
use std::net::IpAddr;
use std::sync::Arc;
use tracing::info;

use crate::config::{MatchOn, RouteRule};

use super::manager::RouteManager;

/// Rule operator for applying and removing route rules
#[derive(Debug, Clone)]
pub struct RuleOperator {
    route_manager: Arc<RouteManager>,
}

impl RuleOperator {
    /// Create a new rule operator
    pub fn new(route_manager: Arc<RouteManager>) -> Self {
        Self { route_manager }
    }

    /// Apply a route rule
    ///
    /// This performs two operations:
    /// 1. Add a policy rule (`ip rule add`) to direct matching traffic to the custom table
    /// 2. Add a route in that table (`ip route add`) to forward traffic via the target interface
    pub async fn apply_rule(
        &self,
        rule: &RouteRule,
        _interface_index: u8,
        table_id: u32,
    ) -> crate::Result<()> {
        let destination = self.get_destination(rule)?;
        let gateway = self.get_gateway(rule)?;
        let interface = rule.route_via.interface.clone();

        // 1. Add policy rule to direct matching traffic to the custom table
        self.route_manager
            .add_policy_rule(Some(destination), None, table_id, rule.priority)
            .await?;

        // 2. Add route in the custom table
        self.route_manager
            .add_route(destination, gateway, &interface, table_id, rule.priority)
            .await?;

        info!(
            "Applied rule '{}' -> {} via {} (table {})",
            rule.name, destination, interface, table_id
        );

        Ok(())
    }

    /// Remove a route rule
    ///
    /// Removes both the policy rule and the route from the custom table.
    pub async fn remove_rule(&self, rule: &RouteRule, table_id: u32) -> crate::Result<()> {
        let destination = self.get_destination(rule)?;

        // 1. Remove policy rule
        self.route_manager
            .remove_policy_rule(Some(destination), None, table_id, rule.priority)
            .await?;

        // 2. Remove route from table
        self.route_manager
            .remove_route(destination, table_id)
            .await?;

        info!("Removed rule '{}' from table {}", rule.name, table_id);

        Ok(())
    }

    /// Get destination from rule
    fn get_destination(&self, rule: &RouteRule) -> crate::Result<IpNetwork> {
        match &rule.match_on {
            MatchOn::Cidr { cidr } => Ok(*cidr),
            MatchOn::Ip { ip } => {
                // Single IP as /32 or /128
                let network = if ip.is_ipv4() {
                    format!("{}/32", ip)
                } else {
                    format!("{}/128", ip)
                };
                network.parse().map_err(|_| {
                    crate::NicAutoSwitchError::InvalidInput("Invalid IP address".to_string())
                })
            }
            MatchOn::Domain { domain } => Err(crate::NicAutoSwitchError::InvalidInput(format!(
                "Domain '{}' needs DNS resolution first",
                domain
            ))),
            MatchOn::DomainPattern { domain_pattern } => {
                Err(crate::NicAutoSwitchError::InvalidInput(format!(
                    "Domain pattern '{}' needs DNS resolution first",
                    domain_pattern
                )))
            }
        }
    }

    /// Get gateway from rule (currently returns None)
    fn get_gateway(&self, _rule: &RouteRule) -> crate::Result<Option<IpAddr>> {
        // TODO: Implement gateway resolution
        Ok(None)
    }

    /// Validate a rule before applying
    pub fn validate_rule(&self, rule: &RouteRule) -> crate::Result<()> {
        rule.validate()?;

        // Additional validation
        if rule.name.is_empty() {
            return Err(crate::NicAutoSwitchError::InvalidInput(
                "Rule name cannot be empty".to_string(),
            ));
        }

        if rule.route_via.interface.is_empty() {
            return Err(crate::NicAutoSwitchError::InvalidInput(
                "Interface cannot be empty".to_string(),
            ));
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
    use crate::config::{MatchOn, RouteVia};
    use std::sync::Arc;

    fn create_test_rule(name: &str, match_on: MatchOn) -> RouteRule {
        RouteRule {
            name: name.to_string(),
            match_on,
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        }
    }

    #[test]
    fn test_rule_operator_new() {
        let manager = RouteManager::default();
        let _operator = RuleOperator::new(Arc::new(manager));
    }

    #[tokio::test]
    async fn test_apply_rule_with_cidr_succeeds() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
        );

        let result = operator.apply_rule(&rule, 0, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_apply_rule_with_ip_succeeds() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test-ip",
            MatchOn::Ip {
                ip: "192.168.1.1".parse().unwrap(),
            },
        );

        let result = operator.apply_rule(&rule, 0, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_apply_rule_with_domain_returns_error() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test-domain",
            MatchOn::Domain {
                domain: "example.com".to_string(),
            },
        );

        let result = operator.apply_rule(&rule, 0, 100).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DNS resolution"));
    }

    #[tokio::test]
    async fn test_remove_rule_with_cidr_succeeds() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
        );

        let result = operator.remove_rule(&rule, 100).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rule_with_valid_rule_succeeds() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
        );

        let result = operator.validate_rule(&rule);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_rule_with_empty_name_returns_error() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
        );

        let result = operator.validate_rule(&rule);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_rule_with_empty_interface_returns_error() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = RouteRule {
            name: "test-rule".to_string(),
            match_on: MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            route_via: RouteVia {
                interface: "".to_string(),
            },
            priority: 100,
        };

        let result = operator.validate_rule(&rule);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Interface cannot be empty")
        );
    }

    #[tokio::test]
    async fn test_apply_rule_with_domain_pattern_returns_error() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = RouteRule {
            name: "test-pattern".to_string(),
            match_on: MatchOn::DomainPattern {
                domain_pattern: "*.example.com".to_string(),
            },
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority: 100,
        };

        let result = operator.apply_rule(&rule, 0, 100).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DNS resolution"));
    }

    #[test]
    fn test_get_destination_with_cidr() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
        );

        let result = operator.get_destination(&rule);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "10.0.0.0/8".parse().unwrap());
    }

    #[test]
    fn test_get_destination_with_ipv4() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test",
            MatchOn::Ip {
                ip: "192.168.1.1".parse().unwrap(),
            },
        );

        let result = operator.get_destination(&rule);
        assert!(result.is_ok());
        let dest = result.unwrap();
        assert!(dest.is_ipv4());
        assert_eq!(dest.prefix(), 32); // Single host route
    }

    #[test]
    fn test_get_destination_with_ipv6() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test",
            MatchOn::Ip {
                ip: "2001:db8::1".parse().unwrap(),
            },
        );

        let result = operator.get_destination(&rule);
        assert!(result.is_ok());
        let dest = result.unwrap();
        assert!(dest.is_ipv6());
        assert_eq!(dest.prefix(), 128); // Single host route
    }

    #[test]
    fn test_get_destination_with_domain_returns_error() {
        let operator = RuleOperator::new(Arc::new(RouteManager::default()));

        let rule = create_test_rule(
            "test",
            MatchOn::Domain {
                domain: "example.com".to_string(),
            },
        );

        let result = operator.get_destination(&rule);
        assert!(result.is_err());
    }
}
