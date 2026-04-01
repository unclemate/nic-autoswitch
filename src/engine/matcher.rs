//! Route rule matching engine
//!
//! This module provides rule matching logic for destination-based routing.

use std::net::IpAddr;

use crate::config::{MatchOn, RouteRule};
use crate::router::DnsResolver;

/// Destination type for matching
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// IP address
    Ip(IpAddr),
    /// Domain name
    Domain(String),
}

impl Destination {
    /// Create a destination from an IP address
    pub fn ip(addr: IpAddr) -> Self {
        Self::Ip(addr)
    }

    /// Create a destination from a domain name
    pub fn domain(name: impl Into<String>) -> Self {
        Self::Domain(name.into())
    }

    /// Get the IP address if this is an IP destination
    pub fn as_ip(&self) -> Option<&IpAddr> {
        match self {
            Destination::Ip(ip) => Some(ip),
            Destination::Domain(_) => None,
        }
    }

    /// Get the domain if this is a domain destination
    pub fn as_domain(&self) -> Option<&str> {
        match self {
            Destination::Ip(_) => None,
            Destination::Domain(d) => Some(d),
        }
    }
}

/// Rule matcher for route selection
#[derive(Debug)]
pub struct RuleMatcher {
    /// DNS resolver for domain matching
    dns_resolver: Option<DnsResolver>,
}

impl RuleMatcher {
    /// Create a new rule matcher
    pub fn new() -> Self {
        Self { dns_resolver: None }
    }

    /// Create a rule matcher with DNS resolver
    pub fn with_dns_resolver(dns_resolver: DnsResolver) -> Self {
        Self {
            dns_resolver: Some(dns_resolver),
        }
    }

    /// Find the best matching rule for a destination
    pub async fn find_matching_rule<'a>(
        &self,
        destination: &Destination,
        rules: &'a [RouteRule],
    ) -> crate::Result<Option<&'a RouteRule>> {
        // Try direct matching first (no DNS required)
        if let Some(rule) = self.find_direct_match(destination, rules) {
            return Ok(Some(rule));
        }

        // If destination is a domain, try DNS resolution
        if let Destination::Domain(domain) = destination
            && let Some(resolver) = &self.dns_resolver
        {
            let addresses = resolver.resolve(domain).await?;
            for addr in addresses {
                let ip_dest = Destination::ip(addr);
                if let Some(rule) = self.find_direct_match(&ip_dest, rules) {
                    return Ok(Some(rule));
                }
            }
        }

        Ok(None)
    }

    /// Find a direct match (no DNS resolution)
    fn find_direct_match<'a>(
        &self,
        destination: &Destination,
        rules: &'a [RouteRule],
    ) -> Option<&'a RouteRule> {
        let mut best_match: Option<&RouteRule> = None;
        let mut best_priority = u32::MAX;

        for rule in rules {
            if self.rule_matches(rule, destination) && rule.priority < best_priority {
                best_match = Some(rule);
                best_priority = rule.priority;
            }
        }

        best_match
    }

    /// Check if a rule matches a destination
    fn rule_matches(&self, rule: &RouteRule, destination: &Destination) -> bool {
        match (&rule.match_on, destination) {
            // CIDR matching
            (MatchOn::Cidr { cidr }, Destination::Ip(ip)) => cidr.contains(*ip),

            // Exact IP matching
            (MatchOn::Ip { ip: rule_ip }, Destination::Ip(dest_ip)) => rule_ip == dest_ip,

            // Domain matching (direct, no DNS)
            (MatchOn::Domain { domain }, Destination::Domain(dest)) => domain == dest,

            // Domain pattern matching (wildcard)
            (MatchOn::DomainPattern { domain_pattern }, Destination::Domain(dest)) => {
                self.matches_wildcard(domain_pattern, dest)
            }

            // No match for other combinations
            _ => false,
        }
    }

    /// Check if a wildcard pattern matches a domain
    fn matches_wildcard(&self, pattern: &str, domain: &str) -> bool {
        // Pattern should start with '*'
        if !pattern.starts_with('*') {
            return false;
        }

        // Get the suffix after '*'
        let suffix = &pattern[1..];

        // Check if domain ends with the suffix
        if suffix.is_empty() {
            return true;
        }

        // Match: domain should end with suffix
        // And should have at least one character before the suffix
        // (unless pattern is just '*')
        if domain.len() < suffix.len() {
            return false;
        }

        let domain_suffix = &domain[domain.len() - suffix.len()..];
        domain_suffix == suffix && (domain.len() > suffix.len() || suffix.is_empty())
    }

    /// Match all rules for a destination (returns all matches, sorted by priority)
    pub async fn find_all_matching_rules<'a>(
        &self,
        destination: &Destination,
        rules: &'a [RouteRule],
    ) -> crate::Result<Vec<&'a RouteRule>> {
        let mut matches = Vec::new();

        // Find direct matches
        for rule in rules {
            if self.rule_matches(rule, destination) {
                matches.push(rule);
            }
        }

        // If destination is a domain, also check DNS-resolved IPs
        if let Destination::Domain(domain) = destination
            && let Some(resolver) = &self.dns_resolver
        {
            let addresses = resolver.resolve(domain).await?;
            for addr in addresses {
                let ip_dest = Destination::ip(addr);
                for rule in rules {
                    if self.rule_matches(rule, &ip_dest) && !matches.contains(&rule) {
                        matches.push(rule);
                    }
                }
            }
        }

        // Sort by priority (lower = higher priority)
        matches.sort_by_key(|r| r.priority);

        Ok(matches)
    }
}

impl Default for RuleMatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MatchOn, RouteVia};

    fn create_test_rule(name: &str, match_on: MatchOn, priority: u32) -> RouteRule {
        RouteRule {
            name: name.to_string(),
            match_on,
            route_via: RouteVia {
                interface: "eth0".to_string(),
            },
            priority,
        }
    }

    // -------------------------------------------------------------------------
    // Destination Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_destination_ip_creates_ip_destination() {
        let dest = Destination::ip("192.168.1.1".parse().unwrap());
        assert!(dest.as_ip().is_some());
        assert!(dest.as_domain().is_none());
    }

    #[test]
    fn test_destination_domain_creates_domain_destination() {
        let dest = Destination::domain("example.com");
        assert!(dest.as_domain().is_some());
        assert!(dest.as_ip().is_none());
    }

    #[test]
    fn test_destination_equality() {
        let dest1 = Destination::ip("192.168.1.1".parse().unwrap());
        let dest2 = Destination::ip("192.168.1.1".parse().unwrap());
        assert_eq!(dest1, dest2);
    }

    // -------------------------------------------------------------------------
    // RuleMatcher Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_rule_matcher_new_creates_matcher() {
        let matcher = RuleMatcher::new();
        assert!(matcher.dns_resolver.is_none());
    }

    #[test]
    fn test_rule_matcher_default() {
        let matcher = RuleMatcher::default();
        assert!(matcher.dns_resolver.is_none());
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_cidr_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "corp-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("10.1.2.3".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "corp-cidr");
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_non_matching_cidr_returns_none() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "corp-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("8.8.8.8".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_exact_ip_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "exact-ip",
            MatchOn::Ip {
                ip: "192.168.1.100".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("192.168.1.100".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "exact-ip");
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_non_matching_ip_returns_none() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "exact-ip",
            MatchOn::Ip {
                ip: "192.168.1.100".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("192.168.1.101".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_domain_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "exact-domain",
            MatchOn::Domain {
                domain: "gitlab.corp.com".to_string(),
            },
            100,
        )];

        let dest = Destination::domain("gitlab.corp.com");
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "exact-domain");
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_domain_pattern_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "wildcard",
            MatchOn::DomainPattern {
                domain_pattern: "*.corp.example.com".to_string(),
            },
            100,
        )];

        let dest = Destination::domain("gitlab.corp.example.com");
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "wildcard");
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_non_matching_domain_returns_none() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "wildcard",
            MatchOn::DomainPattern {
                domain_pattern: "*.corp.example.com".to_string(),
            },
            100,
        )];

        let dest = Destination::domain("example.com");
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_matching_rule_returns_highest_priority() {
        let matcher = RuleMatcher::new();
        let rules = vec![
            create_test_rule(
                "low-priority",
                MatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                200,
            ),
            create_test_rule(
                "high-priority",
                MatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                50,
            ),
        ];

        let dest = Destination::ip("10.1.2.3".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "high-priority");
    }

    // -------------------------------------------------------------------------
    // Wildcard Matching Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_matches_wildcard_with_exact_suffix_match() {
        let matcher = RuleMatcher::new();
        assert!(matcher.matches_wildcard("*.example.com", "www.example.com"));
        assert!(matcher.matches_wildcard("*.example.com", "api.example.com"));
    }

    #[test]
    fn test_matches_wildcard_with_non_matching_suffix() {
        let matcher = RuleMatcher::new();
        assert!(!matcher.matches_wildcard("*.example.com", "example.org"));
        assert!(!matcher.matches_wildcard("*.example.com", "www.example.org"));
    }

    #[test]
    fn test_matches_wildcard_with_just_wildcard() {
        let matcher = RuleMatcher::new();
        assert!(matcher.matches_wildcard("*", "anything.com"));
        assert!(matcher.matches_wildcard("*", "example"));
    }

    #[test]
    fn test_matches_wildcard_with_subdomain() {
        let matcher = RuleMatcher::new();
        assert!(matcher.matches_wildcard("*.corp.example.com", "gitlab.corp.example.com"));
        assert!(matcher.matches_wildcard("*.corp.example.com", "api.prod.corp.example.com"));
    }

    #[test]
    fn test_matches_wildcard_with_short_domain() {
        let matcher = RuleMatcher::new();
        assert!(!matcher.matches_wildcard("*.example.com", "com"));
    }

    // -------------------------------------------------------------------------
    // find_all_matching_rules Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_all_matching_rules_returns_sorted_by_priority() {
        let matcher = RuleMatcher::new();
        let rules = vec![
            create_test_rule(
                "low",
                MatchOn::Cidr {
                    cidr: "10.0.0.0/8".parse().unwrap(),
                },
                300,
            ),
            create_test_rule(
                "medium",
                MatchOn::Cidr {
                    cidr: "10.0.0.0/16".parse().unwrap(),
                },
                200,
            ),
            create_test_rule(
                "high",
                MatchOn::Cidr {
                    cidr: "10.0.0.0/24".parse().unwrap(),
                },
                100,
            ),
        ];

        let dest = Destination::ip("10.0.0.1".parse().unwrap());
        let result = matcher.find_all_matching_rules(&dest, &rules).await;

        assert!(result.is_ok());
        let matches = result.unwrap();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0].name, "high");
        assert_eq!(matches[1].name, "medium");
        assert_eq!(matches[2].name, "low");
    }

    #[tokio::test]
    async fn test_find_all_matching_rules_with_no_matches_returns_empty() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "corp-cidr",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("192.168.1.1".parse().unwrap());
        let result = matcher.find_all_matching_rules(&dest, &rules).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    // -------------------------------------------------------------------------
    // IPv6 Tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_matching_rule_with_ipv6_cidr_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "ipv6-cidr",
            MatchOn::Cidr {
                cidr: "2001:db8::/32".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("2001:db8::1".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "ipv6-cidr");
    }

    #[tokio::test]
    async fn test_find_matching_rule_with_ipv6_exact_ip_returns_match() {
        let matcher = RuleMatcher::new();
        let rules = vec![create_test_rule(
            "ipv6-exact",
            MatchOn::Ip {
                ip: "2001:db8::1".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::ip("2001:db8::1".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &rules).await;

        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "ipv6-exact");
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_matching_rule_empty_rules_returns_none() {
        let matcher = RuleMatcher::new();
        let dest = Destination::ip("10.0.0.1".parse().unwrap());
        let result = matcher.find_matching_rule(&dest, &[]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_find_matching_rule_domain_no_resolver_returns_none() {
        let matcher = RuleMatcher::new(); // No DNS resolver
        let rules = vec![create_test_rule(
            "cidr-rule",
            MatchOn::Cidr {
                cidr: "10.0.0.0/8".parse().unwrap(),
            },
            100,
        )];
        let dest = Destination::domain("example.com");
        let result = matcher.find_matching_rule(&dest, &rules).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_with_dns_resolver_creates_matcher_with_resolver() {
        let resolver = DnsResolver::new().unwrap();
        let matcher = RuleMatcher::with_dns_resolver(resolver);
        assert!(matcher.dns_resolver.is_some());
    }

    #[test]
    fn test_matches_wildcard_without_star_prefix_returns_false() {
        let matcher = RuleMatcher::new();
        assert!(!matcher.matches_wildcard("example.com", "example.com"));
    }

    #[test]
    fn test_matches_wildcard_star_dot_exact_match() {
        let matcher = RuleMatcher::new();
        // "*.example.com" should match "a.example.com" but NOT "example.com"
        assert!(!matcher.matches_wildcard("*.example.com", "example.com"));
    }

    // -------------------------------------------------------------------------
    // DNS resolution path tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_find_matching_rule_domain_with_dns_resolver() {
        let resolver = DnsResolver::new().unwrap();
        let matcher = RuleMatcher::with_dns_resolver(resolver);

        // localhost resolves to 127.0.0.1, create a CIDR rule that matches
        let rules = vec![create_test_rule(
            "localhost-cidr",
            MatchOn::Cidr {
                cidr: "127.0.0.0/8".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::domain("localhost");
        let result = matcher.find_matching_rule(&dest, &rules).await;
        assert!(result.is_ok());
        let matching_rule = result.unwrap();
        assert!(matching_rule.is_some());
        assert_eq!(matching_rule.unwrap().name, "localhost-cidr");
    }

    #[tokio::test]
    async fn test_find_all_matching_rules_with_dns_resolution() {
        let resolver = DnsResolver::new().unwrap();
        let matcher = RuleMatcher::with_dns_resolver(resolver);

        let rules = vec![create_test_rule(
            "localhost-cidr",
            MatchOn::Cidr {
                cidr: "127.0.0.0/8".parse().unwrap(),
            },
            100,
        )];

        let dest = Destination::domain("localhost");
        let result = matcher.find_all_matching_rules(&dest, &rules).await;
        assert!(result.is_ok());
        let matches = result.unwrap();
        assert!(!matches.is_empty());
        assert_eq!(matches[0].name, "localhost-cidr");
    }
}
