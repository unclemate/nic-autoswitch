//! Route table management
//!
//! This module provides routing table operations using rtnetlink.
//! In dry-run mode, all operations log what they would do without modifying the kernel.
//!
//! ## Route/Rule Architecture
//!
//! Each configured interface gets its own routing table (100+index for IPv4, 200+index for IPv6).
//! Policy rules (`ip rule`) direct traffic to the appropriate table:
//!   - Default rules: `to <dest> lookup <table>` (for CIDR-based matching)
//!
//! Routes within each table direct traffic to the correct interface:
//!   - `to <dest>/<prefix> dev <interface>` (no gateway for local subnets)

use std::net::IpAddr;

use futures::stream::TryStreamExt;
use ipnetwork::IpNetwork;
use netlink_packet_route::route::{
    RouteAddress, RouteAttribute, RouteMessage, RouteProtocol, RouteScope, RouteType,
};
use netlink_packet_route::rule::{RuleAction, RuleAttribute, RuleMessage};
use rtnetlink::Handle;
use tracing::{debug, info, warn};

/// Check if a rtnetlink error indicates "entry already exists" (EEXIST, errno 17).
///
/// Treating EEXIST as success makes add operations idempotent.
fn is_already_exists(err: &rtnetlink::Error) -> bool {
    netlink_errno(err) == Some(17)
}

/// Check if a rtnetlink error indicates "no such entry" (ENOENT errno 2 or ENXIO errno 6).
///
/// Treating these as success makes remove operations idempotent.
fn is_no_such_entry(err: &rtnetlink::Error) -> bool {
    matches!(netlink_errno(err), Some(2) | Some(6))
}

/// Extract the errno from a rtnetlink error, if available.
fn netlink_errno(err: &rtnetlink::Error) -> Option<i32> {
    match err {
        rtnetlink::Error::NetlinkError(msg) => msg.code.map(|c| c.get().abs()),
        _ => None,
    }
}

/// Route table ID ranges
pub const IPV4_TABLE_START: u32 = 100;
pub const IPV4_TABLE_END: u32 = 199;
pub const IPV6_TABLE_START: u32 = 200;
pub const IPV6_TABLE_END: u32 = 299;

/// Maximum valid routing table ID (u8 range in netlink header)
const MAX_TABLE_ID: u32 = 255;

/// Convert a table_id to u8 for netlink headers, with overflow check
fn table_id_to_u8(table_id: u32) -> crate::Result<u8> {
    if table_id > MAX_TABLE_ID {
        Err(crate::NicAutoSwitchError::Route(format!(
            "table_id {} exceeds maximum {} (netlink header is u8)",
            table_id, MAX_TABLE_ID
        )))
    } else {
        Ok(table_id as u8)
    }
}

/// Build a `RuleMessage` for policy rule operations (add/remove).
///
/// Consolidates IPv4/IPv6 branch logic into a single function.
fn build_rule_message(
    destination: Option<IpNetwork>,
    source: Option<IpNetwork>,
    table_id: u32,
    priority: u32,
    is_ipv4: bool,
) -> crate::Result<RuleMessage> {
    let mut msg = RuleMessage::default();
    msg.header.table = table_id_to_u8(table_id)?;
    msg.header.action = RuleAction::ToTable;
    msg.header.family = if is_ipv4 {
        netlink_packet_route::AddressFamily::Inet
    } else {
        netlink_packet_route::AddressFamily::Inet6
    };

    if is_ipv4 {
        if let Some(IpNetwork::V4(net)) = destination {
            msg.header.dst_len = net.prefix();
            msg.attributes
                .push(RuleAttribute::Destination(net.ip().into()));
        }
        if let Some(IpNetwork::V4(net)) = source {
            msg.header.src_len = net.prefix();
            msg.attributes.push(RuleAttribute::Source(net.ip().into()));
        }
    } else {
        if let Some(IpNetwork::V6(net)) = destination {
            msg.header.dst_len = net.prefix();
            msg.attributes
                .push(RuleAttribute::Destination(net.ip().into()));
        }
        if let Some(IpNetwork::V6(net)) = source {
            msg.header.src_len = net.prefix();
            msg.attributes.push(RuleAttribute::Source(net.ip().into()));
        }
    }

    msg.attributes.push(RuleAttribute::Priority(priority));

    Ok(msg)
}

/// Route manager for policy routing
#[derive(Debug, Clone)]
pub struct RouteManager {
    /// Base table ID for IPv4
    base_table_id: u32,
    /// Dry-run mode: log but don't execute
    dry_run: bool,
    /// rtnetlink handle (None in dry-run or test mode)
    handle: Option<Handle>,
}

impl RouteManager {
    /// Create a new route manager with rtnetlink connection
    ///
    /// In dry-run mode, no rtnetlink connection is created.
    /// Requires `CAP_NET_ADMIN` when `dry_run = false`.
    pub async fn with_connection(base_table_id: u32, dry_run: bool) -> crate::Result<Self> {
        if !(IPV4_TABLE_START..=IPV4_TABLE_END).contains(&base_table_id) {
            return Err(crate::NicAutoSwitchError::InvalidInput(format!(
                "base_table_id must be between {} and {}",
                IPV4_TABLE_START, IPV4_TABLE_END
            )));
        }

        let handle = if dry_run {
            info!("RouteManager: dry-run mode, skipping rtnetlink connection");
            None
        } else {
            match rtnetlink::new_connection() {
                Ok((connection, handle, _)) => {
                    tokio::spawn(connection);
                    info!("RouteManager: rtnetlink connection established");
                    Some(handle)
                }
                Err(e) => {
                    warn!("RouteManager: failed to connect rtnetlink: {}", e);
                    return Err(crate::NicAutoSwitchError::Network(format!(
                        "Failed to create rtnetlink connection: {}",
                        e
                    )));
                }
            }
        };

        Ok(Self {
            base_table_id,
            dry_run,
            handle,
        })
    }

    /// Create a route manager without connection (for validation / testing)
    pub fn new(base_table_id: u32) -> crate::Result<Self> {
        if !(IPV4_TABLE_START..=IPV4_TABLE_END).contains(&base_table_id) {
            return Err(crate::NicAutoSwitchError::InvalidInput(format!(
                "base_table_id should be between {} and {}",
                IPV4_TABLE_START, IPV4_TABLE_END
            )));
        }

        Ok(Self {
            base_table_id,
            dry_run: true,
            handle: None,
        })
    }

    /// Get the route table ID for an interface (IPv4)
    pub fn table_id_v4(&self, index: u8) -> u32 {
        self.base_table_id + u32::from(index)
    }

    /// Get the route table ID for an interface (IPv6)
    pub fn table_id_v6(&self, index: u8) -> u32 {
        IPV6_TABLE_START + u32::from(index)
    }

    /// Whether dry-run mode is active
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// Get the rtnetlink handle
    pub fn handle(&self) -> Option<&Handle> {
        self.handle.as_ref()
    }

    // -----------------------------------------------------------------------
    // Route operations (high-level builder API)
    // -----------------------------------------------------------------------

    /// Add a route to a custom routing table
    pub async fn add_route(
        &self,
        destination: IpNetwork,
        gateway: Option<IpAddr>,
        interface: &str,
        table_id: u32,
        metric: u32,
    ) -> crate::Result<()> {
        if self.dry_run {
            info!(
                "[DRY-RUN] Would add route: {} dev {} via {:?} (table {}, metric {})",
                destination, interface, gateway, table_id, metric
            );
            return Ok(());
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (daemon may not have CAP_NET_ADMIN or is in dry-run mode)"
                    .into(),
            )
        })?;

        // Resolve interface index
        let if_index = self.resolve_interface_index(interface).await?;

        if destination.is_ipv4() {
            let dest = match destination.ip() {
                IpAddr::V4(v4) => v4,
                _ => unreachable!(),
            };

            let mut req = handle
                .route()
                .add()
                .v4()
                .destination_prefix(dest, destination.prefix())
                .output_interface(if_index)
                .table_id(table_id)
                .protocol(RouteProtocol::Static)
                .scope(RouteScope::Universe)
                .kind(RouteType::Unicast);

            if let Some(IpAddr::V4(gw)) = gateway {
                req = req.gateway(gw);
            }

            // Set metric (priority) via message_mut
            req.message_mut()
                .attributes
                .push(RouteAttribute::Priority(metric));

            match req.execute().await {
                Ok(()) => {}
                Err(e) if is_already_exists(&e) => {
                    debug!(
                        "Route {} dev {} table {} already exists, treating as success",
                        destination, interface, table_id
                    );
                }
                Err(e) => {
                    return Err(crate::NicAutoSwitchError::Route(format!(
                        "Failed to add route {} dev {} table {}: {}",
                        destination, interface, table_id, e
                    )));
                }
            }
        } else {
            let dest = match destination.ip() {
                IpAddr::V6(v6) => v6,
                _ => unreachable!(),
            };

            let mut req = handle
                .route()
                .add()
                .v6()
                .destination_prefix(dest, destination.prefix())
                .output_interface(if_index)
                .table_id(table_id)
                .protocol(RouteProtocol::Static)
                .scope(RouteScope::Universe)
                .kind(RouteType::Unicast);

            if let Some(IpAddr::V6(gw)) = gateway {
                req = req.gateway(gw);
            }

            req.message_mut()
                .attributes
                .push(RouteAttribute::Priority(metric));

            match req.execute().await {
                Ok(()) => {}
                Err(e) if is_already_exists(&e) => {
                    debug!(
                        "IPv6 route {} dev {} table {} already exists, treating as success",
                        destination, interface, table_id
                    );
                }
                Err(e) => {
                    return Err(crate::NicAutoSwitchError::Route(format!(
                        "Failed to add IPv6 route {} dev {} table {}: {}",
                        destination, interface, table_id, e
                    )));
                }
            }
        }

        info!(
            "Added route: {} dev {} table {} (metric {})",
            destination, interface, table_id, metric
        );

        Ok(())
    }

    /// Remove a route from a custom routing table
    pub async fn remove_route(&self, destination: IpNetwork, table_id: u32) -> crate::Result<()> {
        if self.dry_run {
            info!(
                "[DRY-RUN] Would remove route: {} from table {}",
                destination, table_id
            );
            return Ok(());
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        // Build a RouteMessage to identify the route to delete
        let mut msg = RouteMessage::default();
        msg.header.table = table_id_to_u8(table_id)?;
        msg.header.protocol = RouteProtocol::Static;
        msg.header.scope = RouteScope::Universe;
        msg.header.kind = RouteType::Unicast;
        msg.header.destination_prefix_length = destination.prefix();

        // Set destination address
        if destination.is_ipv4() {
            let dest = match destination.ip() {
                IpAddr::V4(v4) => v4,
                _ => unreachable!(),
            };
            msg.attributes
                .push(RouteAttribute::Destination(RouteAddress::Inet(dest)));
        } else {
            let dest = match destination.ip() {
                IpAddr::V6(v6) => v6,
                _ => unreachable!(),
            };
            msg.attributes
                .push(RouteAttribute::Destination(RouteAddress::Inet6(dest)));
        }

        match handle.route().del(msg).execute().await {
            Ok(()) => {}
            Err(e) if is_no_such_entry(&e) => {
                debug!(
                    "Route {} in table {} already absent, treating as success",
                    destination, table_id
                );
            }
            Err(e) => {
                return Err(crate::NicAutoSwitchError::Route(format!(
                    "Failed to remove route {} from table {}: {}",
                    destination, table_id, e
                )));
            }
        }

        info!("Removed route: {} from table {}", destination, table_id);
        Ok(())
    }

    /// Flush all routes in a specific routing table
    pub async fn flush_table(&self, table_id: u32) -> crate::Result<usize> {
        if self.dry_run {
            info!("[DRY-RUN] Would flush routes in table {}", table_id);
            return Ok(0);
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let mut deleted = 0;
        let mut failed = 0;
        let mut req = handle.route().get(rtnetlink::IpVersion::V4);
        req.message_mut().header.table = table_id_to_u8(table_id)?;
        let mut routes = req.execute();

        while let Some(route) = routes.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Route(format!(
                "Failed to list routes in table {}: {}",
                table_id, e
            ))
        })? {
            if let Err(e) = handle.route().del(route).execute().await {
                warn!("Failed to delete route from table {}: {}", table_id, e);
                failed += 1;
            } else {
                deleted += 1;
            }
        }

        if failed > 0 {
            warn!(
                "Flushed table {}: {} deleted, {} failed",
                table_id, deleted, failed
            );
        } else {
            info!("Flushed {} routes from table {}", deleted, table_id);
        }
        Ok(deleted)
    }

    /// Flush all policy rules that reference a specific routing table
    pub async fn flush_table_policy_rules(&self, table_id: u32) -> crate::Result<usize> {
        if self.dry_run {
            info!("[DRY-RUN] Would flush policy rules for table {}", table_id);
            return Ok(0);
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let table_u8 = table_id_to_u8(table_id)?;
        let mut count = 0;

        // Enumerate IPv4 rules matching this table
        let mut rules = handle.rule().get(rtnetlink::IpVersion::V4).execute();
        while let Some(rule) = rules.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Route(format!(
                "Failed to list policy rules for table {}: {}",
                table_id, e
            ))
        })? {
            if rule.header.table == table_u8 {
                if let Err(e) = handle.rule().del(rule).execute().await {
                    warn!(
                        "Failed to delete policy rule from table {}: {}",
                        table_id, e
                    );
                } else {
                    count += 1;
                }
            }
        }

        // Also check IPv6 rules
        let mut rules = handle.rule().get(rtnetlink::IpVersion::V6).execute();
        while let Some(rule) = rules.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Route(format!(
                "Failed to list IPv6 policy rules for table {}: {}",
                table_id, e
            ))
        })? {
            if rule.header.table == table_u8 {
                if let Err(e) = handle.rule().del(rule).execute().await {
                    warn!(
                        "Failed to delete IPv6 policy rule from table {}: {}",
                        table_id, e
                    );
                } else {
                    count += 1;
                }
            }
        }

        info!(
            "Flushed {} policy rules referencing table {}",
            count, table_id
        );
        Ok(count)
    }

    /// Flush both routes and policy rules for a table
    pub async fn flush_table_complete(&self, table_id: u32) -> crate::Result<usize> {
        let route_count = self.flush_table(table_id).await?;
        let rule_count = self.flush_table_policy_rules(table_id).await?;
        Ok(route_count + rule_count)
    }

    // -----------------------------------------------------------------------
    // Policy rule operations (high-level builder API)
    // -----------------------------------------------------------------------

    /// Add a policy routing rule (`ip rule add`)
    pub async fn add_policy_rule(
        &self,
        destination: Option<IpNetwork>,
        source: Option<IpNetwork>,
        table_id: u32,
        priority: u32,
    ) -> crate::Result<()> {
        if self.dry_run {
            info!(
                "[DRY-RUN] Would add policy rule: dst={:?}, src={:?}, table={}, priority={}",
                destination, source, table_id, priority
            );
            return Ok(());
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        // Determine IPv4 vs IPv6
        let is_ipv4 = destination
            .map(|d| d.is_ipv4())
            .or_else(|| source.map(|s| s.is_ipv4()))
            .unwrap_or(true);

        if is_ipv4 {
            let mut req = handle.rule().add().v4().action(RuleAction::ToTable);

            if let Some(IpNetwork::V4(net)) = destination {
                req = req.destination_prefix(net.ip(), net.prefix());
            }
            if let Some(IpNetwork::V4(net)) = source {
                req = req.source_prefix(net.ip(), net.prefix());
            }

            req = req.table_id(table_id).priority(priority);
            match req.execute().await {
                Ok(()) => {}
                Err(e) if is_already_exists(&e) => {
                    debug!(
                        "IPv4 policy rule (table {}, priority {}) already exists, treating as success",
                        table_id, priority
                    );
                }
                Err(e) => {
                    return Err(crate::NicAutoSwitchError::Route(format!(
                        "Failed to add IPv4 policy rule (table {}, priority {}): {}",
                        table_id, priority, e
                    )));
                }
            }
        } else {
            let mut req = handle.rule().add().v6().action(RuleAction::ToTable);
            if let Some(IpNetwork::V6(net)) = destination {
                req = req.destination_prefix(net.ip(), net.prefix());
            }
            if let Some(IpNetwork::V6(net)) = source {
                req = req.source_prefix(net.ip(), net.prefix());
            }

            req = req.table_id(table_id).priority(priority);
            match req.execute().await {
                Ok(()) => {}
                Err(e) if is_already_exists(&e) => {
                    debug!(
                        "IPv6 policy rule (table {}, priority {}) already exists, treating as success",
                        table_id, priority
                    );
                }
                Err(e) => {
                    return Err(crate::NicAutoSwitchError::Route(format!(
                        "Failed to add IPv6 policy rule (table {}, priority {}): {}",
                        table_id, priority, e
                    )));
                }
            }
        }

        debug!(
            "Added policy rule: dst={:?}, src={:?}, table={}, priority={}",
            destination, source, table_id, priority
        );
        Ok(())
    }

    /// Remove a policy routing rule
    pub async fn remove_policy_rule(
        &self,
        destination: Option<IpNetwork>,
        source: Option<IpNetwork>,
        table_id: u32,
        priority: u32,
    ) -> crate::Result<()> {
        if self.dry_run {
            info!(
                "[DRY-RUN] Would remove policy rule: table={}, priority={}",
                table_id, priority
            );
            return Ok(());
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let is_ipv4 = destination
            .map(|d| d.is_ipv4())
            .or_else(|| source.map(|s| s.is_ipv4()))
            .unwrap_or(true);

        let ip_ver = if is_ipv4 { "IPv4" } else { "IPv6" };
        let rule_msg = build_rule_message(destination, source, table_id, priority, is_ipv4)?;

        match handle.rule().del(rule_msg).execute().await {
            Ok(()) => {}
            Err(e) if is_no_such_entry(&e) => {
                debug!(
                    "{} policy rule (table {}, priority {}) already absent, treating as success",
                    ip_ver, table_id, priority
                );
            }
            Err(e) => {
                return Err(crate::NicAutoSwitchError::Route(format!(
                    "Failed to remove {} policy rule (table {}, priority {}): {}",
                    ip_ver, table_id, priority, e
                )));
            }
        }

        debug!(
            "Removed policy rule: table={}, priority={}",
            table_id, priority
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Interface lookup
    // -----------------------------------------------------------------------

    /// Resolve interface name to rtnetlink interface index
    pub async fn resolve_interface_index(&self, name: &str) -> crate::Result<u32> {
        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let mut links = handle.link().get().match_name(name.to_string()).execute();
        match links.try_next().await {
            Ok(Some(link)) => Ok(link.header.index),
            Ok(None) => Err(crate::NicAutoSwitchError::Route(format!(
                "Interface '{}' not found",
                name
            ))),
            Err(e) => Err(crate::NicAutoSwitchError::Route(format!(
                "Failed to query interface '{}': {}",
                name, e
            ))),
        }
    }

    // -----------------------------------------------------------------------
    // Query operations
    // -----------------------------------------------------------------------

    /// Check if any routes exist in a specific table
    pub async fn route_exists(&self, destination: IpNetwork, table_id: u32) -> crate::Result<bool> {
        if self.dry_run {
            debug!(
                "[DRY-RUN] Checking route {} in table {} (stub: false)",
                destination, table_id
            );
            return Ok(false);
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let ip_version = if destination.is_ipv4() {
            rtnetlink::IpVersion::V4
        } else {
            rtnetlink::IpVersion::V6
        };
        let mut req = handle.route().get(ip_version);
        req.message_mut().header.table = table_id_to_u8(table_id)?;
        let mut routes = req.execute();

        while let Some(route) = routes.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Route(format!("Failed to query routes: {}", e))
        })? {
            // Check destination prefix length
            if route.header.destination_prefix_length != destination.prefix() {
                continue;
            }
            // Check destination address match
            for attr in &route.attributes {
                if let RouteAttribute::Destination(addr) = attr {
                    let matches = match (addr, &destination.ip()) {
                        (RouteAddress::Inet(v4), IpAddr::V4(dest)) => v4 == dest,
                        (RouteAddress::Inet6(v6), IpAddr::V6(dest)) => v6 == dest,
                        _ => false,
                    };
                    if matches {
                        return Ok(true);
                    }
                }
            }
        }

        Ok(false)
    }

    /// Flush all routes for an interface in a specific table
    pub async fn flush_interface_routes(
        &self,
        interface: &str,
        table_id: u32,
    ) -> crate::Result<usize> {
        if self.dry_run {
            info!(
                "[DRY-RUN] Would flush routes for interface {} in table {}",
                interface, table_id
            );
            return Ok(0);
        }

        let handle = self.handle.as_ref().ok_or_else(|| {
            crate::NicAutoSwitchError::Route(
                "No rtnetlink connection (check CAP_NET_ADMIN or dry-run mode)".into(),
            )
        })?;

        let if_index = self.resolve_interface_index(interface).await?;
        let mut count = 0;

        let mut req = handle.route().get(rtnetlink::IpVersion::V4);
        req.message_mut().header.table = table_id_to_u8(table_id)?;
        let mut routes = req.execute();

        while let Some(route) = routes.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Route(format!(
                "Failed to list routes for interface {} in table {}: {}",
                interface, table_id, e
            ))
        })? {
            // Check if route uses this interface
            let matches_oif = route.attributes.iter().any(|attr| {
                matches!(
                    attr,
                    RouteAttribute::Oif(idx) if *idx == if_index
                )
            });

            if matches_oif {
                if let Err(e) = handle.route().del(route).execute().await {
                    warn!(
                        "Failed to delete route for {} in table {}: {}",
                        interface, table_id, e
                    );
                } else {
                    count += 1;
                }
            }
        }

        info!(
            "Flushed {} routes for interface {} in table {}",
            count, interface, table_id
        );
        Ok(count)
    }

    /// Get base table ID
    pub fn base_table_id(&self) -> u32 {
        self.base_table_id
    }
}

impl Default for RouteManager {
    fn default() -> Self {
        Self {
            base_table_id: IPV4_TABLE_START,
            dry_run: true,
            handle: None,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_manager_new_with_valid_id() {
        let result = RouteManager::new(100);
        assert!(result.is_ok());
        let manager = result.unwrap();
        assert_eq!(manager.base_table_id(), 100);
        assert!(manager.is_dry_run());
    }

    #[test]
    fn test_route_manager_new_with_invalid_low_id() {
        let result = RouteManager::new(50);
        assert!(result.is_err());
    }

    #[test]
    fn test_route_manager_new_with_invalid_high_id() {
        let result = RouteManager::new(300);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_id_v4_calculation() {
        let manager = RouteManager::new(100).unwrap();
        assert_eq!(manager.table_id_v4(0), 100);
        assert_eq!(manager.table_id_v4(1), 101);
        assert_eq!(manager.table_id_v4(10), 110);
    }

    #[test]
    fn test_table_id_v6_calculation() {
        let manager = RouteManager::new(100).unwrap();
        assert_eq!(manager.table_id_v6(0), 200);
        assert_eq!(manager.table_id_v6(1), 201);
        assert_eq!(manager.table_id_v6(10), 210);
    }

    #[test]
    fn test_route_manager_default() {
        let manager = RouteManager::default();
        assert_eq!(manager.base_table_id(), IPV4_TABLE_START);
        assert!(manager.is_dry_run());
        assert!(manager.handle.is_none());
    }

    #[tokio::test]
    async fn test_add_route_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "192.168.1.0/24".parse().unwrap();
        let gateway: IpAddr = "192.168.1.1".parse().unwrap();
        let result = manager
            .add_route(dest, Some(gateway), "eth0", 100, 100)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_route_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "192.168.1.0/24".parse().unwrap();
        let result = manager.remove_route(dest, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_flush_table_returns_zero_for_dry_run() {
        let manager = RouteManager::default();
        let result = manager.flush_table(100).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_add_policy_rule_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "10.0.0.0/8".parse().unwrap();
        let result = manager.add_policy_rule(Some(dest), None, 100, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_policy_rule_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "10.0.0.0/8".parse().unwrap();
        let result = manager.remove_policy_rule(Some(dest), None, 100, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_route_without_handle_returns_error() {
        let manager = RouteManager {
            dry_run: false,
            ..RouteManager::default()
        };
        let dest: IpNetwork = "192.168.1.0/24".parse().unwrap();
        let result = manager.add_route(dest, None, "eth0", 100, 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_interface_index_without_handle_returns_error() {
        let manager = RouteManager::default();
        let result = manager.resolve_interface_index("eth0").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_flush_interface_routes_returns_zero_for_dry_run() {
        let manager = RouteManager::default();
        let result = manager.flush_interface_routes("eth0", 100).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_add_ipv6_route_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "::/0".parse().unwrap();
        let result = manager.add_route(dest, None, "wlan0", 200, 100).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_policy_rule_ipv6_succeeds_for_dry_run() {
        let manager = RouteManager::default();
        let dest: IpNetwork = "2001:db8::/32".parse().unwrap();
        let result = manager.add_policy_rule(Some(dest), None, 200, 100).await;
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // table_id overflow validation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_id_to_u8_valid() {
        assert_eq!(super::table_id_to_u8(100).unwrap(), 100);
        assert_eq!(super::table_id_to_u8(255).unwrap(), 255);
        assert_eq!(super::table_id_to_u8(0).unwrap(), 0);
    }

    #[test]
    fn test_table_id_to_u8_overflow_returns_error() {
        let result = super::table_id_to_u8(256);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn test_flush_table_with_overflow_id_returns_error() {
        let manager = RouteManager {
            dry_run: false,
            ..RouteManager::default()
        };
        let result = manager.flush_table(300).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_flush_table_policy_rules_dry_run_returns_zero() {
        let manager = RouteManager::default();
        let result = manager.flush_table_policy_rules(100).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_flush_table_complete_dry_run_returns_zero() {
        let manager = RouteManager::default();
        let result = manager.flush_table_complete(100).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }
}
