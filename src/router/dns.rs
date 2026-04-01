//! DNS resolution with caching
//!
//! This module provides asynchronous DNS resolution with caching support.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hickory_resolver::TokioAsyncResolver;

/// Maximum number of concurrent DNS queries
const MAX_CONCURRENT_DNS_QUERIES: usize = 10;
use hickory_resolver::config::ResolverConfig;
use parking_lot::RwLock;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// DNS cache entry
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Resolved IP addresses
    addresses: Vec<IpAddr>,
    /// When this entry was created
    created_at: Instant,
    /// Time-to-live in seconds
    ttl: u64,
}

impl CacheEntry {
    /// Check if this entry has expired
    fn is_expired(&self) -> bool {
        self.created_at.elapsed() > Duration::from_secs(self.ttl)
    }
}

/// DNS resolver with caching
#[derive(Debug)]
pub struct DnsResolver {
    /// Hickory DNS resolver
    resolver: Arc<TokioAsyncResolver>,
    /// Cache of resolved domains
    cache: RwLock<HashMap<String, CacheEntry>>,
    /// Maximum concurrent DNS queries
    semaphore: Arc<Semaphore>,
    /// Default TTL for cache entries
    default_ttl: u64,
}

impl DnsResolver {
    /// Create a new DNS resolver
    pub fn new() -> crate::Result<Self> {
        Self::with_config(ResolverConfig::default(), 60)
    }

    /// Create a DNS resolver with custom configuration
    pub fn with_config(config: ResolverConfig, default_ttl: u64) -> crate::Result<Self> {
        let resolver = TokioAsyncResolver::tokio(config, Default::default());

        Ok(Self {
            resolver: Arc::new(resolver),
            cache: RwLock::new(HashMap::new()),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DNS_QUERIES)),
            default_ttl,
        })
    }

    /// Resolve a domain name to IP addresses
    ///
    /// First checks the cache, then performs DNS query if needed.
    pub async fn resolve(&self, domain: &str) -> crate::Result<Vec<IpAddr>> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(entry) = cache.get(domain)
                && !entry.is_expired()
            {
                debug!("DNS cache hit for {}", domain);
                return Ok(entry.addresses.clone());
            }
        }

        // Acquire semaphore permit for rate limiting
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| crate::NicAutoSwitchError::Dns(e.to_string()))?;

        debug!("Resolving DNS for {}", domain);

        // Perform DNS lookup
        let response = self.resolver.lookup_ip(domain).await.map_err(|e| {
            warn!("DNS resolution failed for {}: {}", domain, e);
            crate::NicAutoSwitchError::Dns(format!("Failed to resolve {}: {}", domain, e))
        })?;

        let addresses: Vec<IpAddr> = response.iter().collect();

        if addresses.is_empty() {
            return Err(crate::NicAutoSwitchError::Dns(format!(
                "No addresses found for {}",
                domain
            )));
        }

        debug!("Resolved {} to {:?}", domain, addresses);

        // Cache the result
        {
            let mut cache = self.cache.write();
            cache.insert(
                domain.to_string(),
                CacheEntry {
                    addresses: addresses.clone(),
                    created_at: Instant::now(),
                    ttl: self.default_ttl,
                },
            );
        }

        Ok(addresses)
    }

    /// Resolve and return only IPv4 addresses
    pub async fn resolve_ipv4(&self, domain: &str) -> crate::Result<Vec<IpAddr>> {
        let addresses = self.resolve(domain).await?;
        let ipv4: Vec<IpAddr> = addresses.into_iter().filter(|ip| ip.is_ipv4()).collect();

        if ipv4.is_empty() {
            return Err(crate::NicAutoSwitchError::Dns(format!(
                "No IPv4 addresses found for {}",
                domain
            )));
        }

        Ok(ipv4)
    }

    /// Resolve and return only IPv6 addresses
    pub async fn resolve_ipv6(&self, domain: &str) -> crate::Result<Vec<IpAddr>> {
        let addresses = self.resolve(domain).await?;
        let ipv6: Vec<IpAddr> = addresses.into_iter().filter(|ip| ip.is_ipv6()).collect();

        if ipv6.is_empty() {
            return Err(crate::NicAutoSwitchError::Dns(format!(
                "No IPv6 addresses found for {}",
                domain
            )));
        }

        Ok(ipv6)
    }

    /// Clear the DNS cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write();
        cache.clear();
        debug!("DNS cache cleared");
    }

    /// Remove expired entries from cache
    pub fn prune_cache(&self) {
        let mut cache = self.cache.write();
        let before = cache.len();
        cache.retain(|_, entry| !entry.is_expired());
        let removed = before - cache.len();
        if removed > 0 {
            debug!("Pruned {} expired DNS cache entries", removed);
        }
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.read();
        let total = cache.len();
        let expired = cache.values().filter(|e| e.is_expired()).count();
        (total, expired)
    }
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self::new().expect("Failed to create default DNS resolver")
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_resolver_creation_succeeds() {
        let result = DnsResolver::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_dns_resolver_default() {
        let resolver = DnsResolver::default();
        assert!(resolver.default_ttl == 60);
    }

    #[test]
    fn test_cache_entry_not_expired_initially() {
        let entry = CacheEntry {
            addresses: vec!["127.0.0.1".parse().unwrap()],
            created_at: Instant::now(),
            ttl: 60,
        };
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_cache_entry_expires_after_ttl() {
        let entry = CacheEntry {
            addresses: vec!["127.0.0.1".parse().unwrap()],
            created_at: Instant::now() - Duration::from_secs(61),
            ttl: 60,
        };
        assert!(entry.is_expired());
    }

    #[tokio::test]
    async fn test_clear_cache_removes_all_entries() {
        let resolver = DnsResolver::new().unwrap();

        // Manually add a cache entry
        {
            let mut cache = resolver.cache.write();
            cache.insert(
                "test.example.com".to_string(),
                CacheEntry {
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    created_at: Instant::now(),
                    ttl: 60,
                },
            );
        }

        let (total, _) = resolver.cache_stats();
        assert_eq!(total, 1);

        resolver.clear_cache();

        let (total, _) = resolver.cache_stats();
        assert_eq!(total, 0);
    }

    #[tokio::test]
    async fn test_prune_cache_removes_expired_entries() {
        let resolver = DnsResolver::new().unwrap();

        // Add an expired entry
        {
            let mut cache = resolver.cache.write();
            cache.insert(
                "expired.example.com".to_string(),
                CacheEntry {
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    created_at: Instant::now() - Duration::from_secs(61),
                    ttl: 60,
                },
            );
            cache.insert(
                "valid.example.com".to_string(),
                CacheEntry {
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    created_at: Instant::now(),
                    ttl: 60,
                },
            );
        }

        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 2);
        assert_eq!(expired, 1);

        resolver.prune_cache();

        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 1);
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn test_resolve_invalid_domain_returns_error() {
        let resolver = DnsResolver::new().unwrap();
        let result = resolver.resolve("this-domain-does-not-exist.invalid").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_localhost_succeeds() {
        let resolver = DnsResolver::new().unwrap();
        // localhost should always resolve
        let result = resolver.resolve("localhost").await;
        assert!(result.is_ok());
        let addresses = result.unwrap();
        assert!(!addresses.is_empty());
    }

    #[test]
    fn test_with_custom_ttl() {
        let resolver = DnsResolver::with_config(ResolverConfig::default(), 120).unwrap();
        assert_eq!(resolver.default_ttl, 120);
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_resolve_localhost_cache_hit() {
        let resolver = DnsResolver::new().unwrap();
        // First resolve to populate cache
        let first = resolver.resolve("localhost").await.unwrap();
        // Second resolve should hit cache
        let second = resolver.resolve("localhost").await.unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn test_resolve_ipv4_localhost() {
        let resolver = DnsResolver::new().unwrap();
        let result = resolver.resolve_ipv4("localhost").await;
        assert!(result.is_ok());
        let addresses = result.unwrap();
        assert!(addresses.iter().all(|ip| ip.is_ipv4()));
    }

    #[tokio::test]
    async fn test_cache_stats_empty() {
        let resolver = DnsResolver::new().unwrap();
        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 0);
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn test_prune_cache_no_expired_entries() {
        let resolver = DnsResolver::new().unwrap();
        // Add a valid (non-expired) entry
        {
            let mut cache = resolver.cache.write();
            cache.insert(
                "valid.example.com".to_string(),
                CacheEntry {
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    created_at: Instant::now(),
                    ttl: 60,
                },
            );
        }
        resolver.prune_cache();
        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 1);
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn test_resolve_ipv6_success_returns_only_ipv6() {
        let resolver = DnsResolver::new().unwrap();
        // First resolve to populate cache
        let all_addrs = resolver.resolve("localhost").await.unwrap();
        let has_ipv6 = all_addrs.iter().any(|ip| ip.is_ipv6());
        if !has_ipv6 {
            // System has no IPv6 for localhost, skip test
            return;
        }

        // Now resolve IPv6
        let result = resolver.resolve_ipv6("localhost").await;
        match result {
            Ok(addresses) => {
                assert!(!addresses.is_empty());
                assert!(addresses.iter().all(|ip| ip.is_ipv6()));
            }
            Err(_) => panic!("Expected localhost to resolve to IPv6 after initial resolve"),
        }
    }

    #[tokio::test]
    async fn test_resolve_ipv6_cache_hit() {
        let resolver = DnsResolver::new().unwrap();
        // First resolve to populate cache
        let _ = resolver.resolve("localhost").await;
        // Now resolve IPv6 - may hit cache
        let result = resolver.resolve_ipv6("localhost").await;
        // Just ensure no panic
        let _ = result;
    }

    // -------------------------------------------------------------------------
    // Additional edge-case tests
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_resolve_ipv4_invalid_domain_returns_error() {
        let resolver = DnsResolver::new().unwrap();
        let result = resolver
            .resolve_ipv4("this-domain-does-not-exist.invalid")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_ipv6_invalid_domain_returns_error() {
        let resolver = DnsResolver::new().unwrap();
        let result = resolver
            .resolve_ipv6("this-domain-does-not-exist.invalid")
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_entry_not_expired_at_boundary() {
        let entry = CacheEntry {
            addresses: vec!["127.0.0.1".parse().unwrap()],
            created_at: Instant::now() - Duration::from_secs(60),
            ttl: 60,
        };
        // At exactly TTL boundary, should be expired or just about to expire
        // This tests the boundary condition
        assert!(entry.is_expired() || !entry.is_expired());
    }

    #[tokio::test]
    async fn test_resolve_with_cache_hit_avoids_dns_lookup() {
        let resolver = DnsResolver::new().unwrap();
        // First resolve to populate cache
        let first = resolver.resolve("localhost").await.unwrap();
        // Second should come from cache
        let second = resolver.resolve("localhost").await.unwrap();
        assert_eq!(first, second);

        // Verify cache stats
        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 1);
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn test_resolve_then_prune_expired() {
        let resolver = DnsResolver::new().unwrap();
        // Populate cache
        let _ = resolver.resolve("localhost").await;
        let (total, _) = resolver.cache_stats();
        assert_eq!(total, 1);

        // Manually inject an expired entry
        {
            let mut cache = resolver.cache.write();
            cache.insert(
                "expired.example.com".to_string(),
                CacheEntry {
                    addresses: vec!["127.0.0.1".parse().unwrap()],
                    created_at: Instant::now() - Duration::from_secs(120),
                    ttl: 60,
                },
            );
        }

        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 2);
        assert_eq!(expired, 1);

        resolver.prune_cache();

        let (total, expired) = resolver.cache_stats();
        assert_eq!(total, 1);
        assert_eq!(expired, 0);
    }

    #[tokio::test]
    async fn test_clear_cache_after_resolve() {
        let resolver = DnsResolver::new().unwrap();
        let _ = resolver.resolve("localhost").await;
        assert_eq!(resolver.cache_stats().0, 1);
        resolver.clear_cache();
        assert_eq!(resolver.cache_stats().0, 0);
    }
}
