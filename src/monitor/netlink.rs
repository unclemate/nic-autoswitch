//! Netlink monitoring using rtnetlink
//!
//! This module provides network interface monitoring via netlink sockets.
//! It tracks link state changes (up/down) using a polling approach:
//! callers invoke `poll_changes()` periodically to detect interface state
//! transitions compared to the previous snapshot.

use futures::stream::{Stream, TryStreamExt};
use netlink_packet_route::link::{LinkAttribute, LinkFlag, LinkMessage};
use rtnetlink::Handle;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::{debug, info, warn};

use super::events::{InterfaceChange, NetworkEvent};
use super::state::{InterfaceInfo, InterfaceType};

/// Internal snapshot of a link's state
#[derive(Debug, Clone)]
struct LinkSnapshot {
    name: String,
    is_up: bool,
}

/// Netlink monitor for network interface changes
///
/// Uses polling to detect interface state transitions. On each call to
/// `poll_changes()`, the current state is compared to the previous snapshot
/// and `InterfaceChanged` events are emitted for any transitions.
pub struct NetlinkMonitor {
    /// rtnetlink handle (None if connection failed or no permissions)
    handle: Option<Handle>,
    /// Previous interface state snapshot for change detection
    prev_state: HashMap<u32, LinkSnapshot>,
    /// Pending events not yet consumed by the Stream
    pending: Vec<NetworkEvent>,
}

impl NetlinkMonitor {
    /// Create a new netlink monitor
    ///
    /// Establishes a long-lived rtnetlink connection for reuse.
    /// Falls back to a disconnected state if the connection fails.
    pub async fn new() -> crate::Result<Self> {
        info!("Initializing netlink monitor");

        let handle = match rtnetlink::new_connection() {
            Ok((connection, handle, _)) => {
                tokio::spawn(connection);
                info!("Netlink monitor: rtnetlink connection established");
                Some(handle)
            }
            Err(e) => {
                warn!("Netlink monitor: rtnetlink connection failed: {}", e);
                None
            }
        };

        let mut monitor = Self {
            handle,
            prev_state: HashMap::new(),
            pending: Vec::new(),
        };

        // Take initial snapshot
        if let Ok(interfaces) = monitor.snapshot_interfaces().await {
            for (index, snap) in &interfaces {
                debug!(
                    "Initial interface: {} (index={}, up={})",
                    snap.name, index, snap.is_up
                );
            }
            info!(
                "Netlink monitor initialized with {} interfaces",
                interfaces.len()
            );
            monitor.prev_state = interfaces;
        }

        Ok(monitor)
    }

    /// Poll for interface changes since the last call
    ///
    /// Compares the current interface state against the previous snapshot
    /// and returns `NetworkEvent::InterfaceChanged` for any transitions.
    pub async fn poll_changes(&mut self) -> crate::Result<Vec<NetworkEvent>> {
        let current = self.snapshot_interfaces().await?;
        let mut events = Vec::new();

        // Detect changes and new interfaces
        for (index, current_snap) in &current {
            match self.prev_state.get(index) {
                Some(prev) => {
                    if prev.is_up != current_snap.is_up {
                        let change = if current_snap.is_up {
                            InterfaceChange::Up
                        } else {
                            InterfaceChange::Down
                        };
                        debug!(
                            "Interface {} state changed: {} -> {}",
                            current_snap.name, prev.is_up, current_snap.is_up
                        );
                        events.push(NetworkEvent::InterfaceChanged {
                            interface: current_snap.name.clone(),
                            change,
                        });
                    }
                }
                None => {
                    debug!(
                        "New interface detected: {} (up={})",
                        current_snap.name, current_snap.is_up
                    );
                    if current_snap.is_up {
                        events.push(NetworkEvent::InterfaceChanged {
                            interface: current_snap.name.clone(),
                            change: InterfaceChange::Up,
                        });
                    }
                }
            }
        }

        // Detect removed interfaces
        for (index, prev_snap) in &self.prev_state {
            if !current.contains_key(index) {
                debug!("Interface removed: {}", prev_snap.name);
                events.push(NetworkEvent::InterfaceChanged {
                    interface: prev_snap.name.clone(),
                    change: InterfaceChange::Removed,
                });
            }
        }

        // Buffer events for Stream consumption
        self.pending.extend(events.clone());
        self.prev_state = current;

        Ok(events)
    }

    /// Take a snapshot of all current interfaces using the shared handle
    async fn snapshot_interfaces(&self) -> crate::Result<HashMap<u32, LinkSnapshot>> {
        let handle = match self.handle.as_ref() {
            Some(h) => h,
            None => {
                warn!("Netlink monitor: no rtnetlink handle available");
                return Ok(HashMap::new());
            }
        };

        let mut result = HashMap::new();
        let mut links = handle.link().get().execute();

        while let Some(link) = links.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Network(format!("Failed to enumerate links: {}", e))
        })? {
            let snapshot = Self::extract_link_snapshot(&link);
            result.insert(link.header.index, snapshot);
        }

        Ok(result)
    }

    /// Extract link state from a LinkMessage
    fn extract_link_snapshot(link: &LinkMessage) -> LinkSnapshot {
        let is_up = link.header.flags.contains(&LinkFlag::Up)
            && link.header.flags.contains(&LinkFlag::Running);

        let name = link
            .attributes
            .iter()
            .find_map(|attr| match attr {
                LinkAttribute::IfName(name) => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| format!("if{}", link.header.index));

        LinkSnapshot { name, is_up }
    }

    /// Get current interface list as InterfaceInfo
    pub async fn get_interfaces(&self) -> crate::Result<Vec<InterfaceInfo>> {
        debug!("Querying current interfaces");

        let handle = match self.handle.as_ref() {
            Some(h) => h,
            None => {
                warn!("Netlink monitor: no rtnetlink handle available");
                return Ok(Vec::new());
            }
        };

        let mut result = Vec::new();
        let mut links = handle.link().get().execute();

        while let Some(link) = links.try_next().await.map_err(|e| {
            crate::NicAutoSwitchError::Network(format!("Failed to enumerate links: {}", e))
        })? {
            result.push(Self::link_to_interface_info(&link));
        }

        Ok(result)
    }

    /// Get interface names that are currently UP
    pub async fn get_active_interface_names(&self) -> crate::Result<Vec<String>> {
        let interfaces = self.get_interfaces().await?;
        Ok(interfaces
            .into_iter()
            .filter(|i| i.is_up)
            .map(|i| i.name)
            .collect())
    }

    /// Convert a LinkMessage to InterfaceInfo
    fn link_to_interface_info(link: &LinkMessage) -> InterfaceInfo {
        let name = link
            .attributes
            .iter()
            .find_map(|attr| match attr {
                LinkAttribute::IfName(name) => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| format!("if{}", link.header.index));

        let is_up = link.header.flags.contains(&LinkFlag::Up);

        // Determine interface type (best effort based on naming convention)
        let interface_type = if name.starts_with("wl") || name.starts_with("wlan") {
            InterfaceType::Wlan
        } else {
            InterfaceType::Lan
        };

        let mut info = InterfaceInfo::new(name, interface_type);
        info.is_up = is_up;
        info
    }
}

impl Stream for NetlinkMonitor {
    type Item = NetworkEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let monitor = self.get_mut();
        if !monitor.pending.is_empty() {
            Poll::Ready(Some(monitor.pending.remove(0)))
        } else {
            // No events ready; caller should call poll_changes() to generate events
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_netlink_monitor_creation() {
        let result = NetlinkMonitor::new().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_interfaces_returns_vec() {
        let monitor = NetlinkMonitor::new().await.unwrap();
        let result = monitor.get_interfaces().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_netlink_monitor_stream_poll_returns_pending_when_empty() {
        use std::task::{RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        let mut monitor = NetlinkMonitor {
            handle: None,
            prev_state: HashMap::new(),
            pending: Vec::new(),
        };
        let poll = Pin::new(&mut monitor).poll_next(&mut cx);
        assert!(matches!(poll, Poll::Pending));
    }

    #[test]
    fn test_netlink_monitor_stream_returns_pending_event() {
        use std::task::{RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
        let raw = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw) };
        let mut cx = Context::from_waker(&waker);

        let mut monitor = NetlinkMonitor {
            handle: None,
            prev_state: HashMap::new(),
            pending: vec![NetworkEvent::InterfaceChanged {
                interface: "eth0".to_string(),
                change: InterfaceChange::Up,
            }],
        };
        let poll = Pin::new(&mut monitor).poll_next(&mut cx);
        assert!(matches!(poll, Poll::Ready(Some(_))));
    }

    #[test]
    fn test_link_to_interface_info_extracts_name() {
        let mut link = LinkMessage::default();
        link.header.index = 2;
        link.header.flags.push(LinkFlag::Up);
        link.attributes
            .push(LinkAttribute::IfName("eth0".to_string()));

        let info = NetlinkMonitor::link_to_interface_info(&link);
        assert_eq!(info.name, "eth0");
        assert!(info.is_up);
        assert_eq!(info.interface_type, InterfaceType::Lan);
    }

    #[test]
    fn test_link_to_interface_info_wlan_type() {
        let mut link = LinkMessage::default();
        link.header.index = 3;
        link.attributes
            .push(LinkAttribute::IfName("wlan0".to_string()));

        let info = NetlinkMonitor::link_to_interface_info(&link);
        assert_eq!(info.interface_type, InterfaceType::Wlan);
    }

    #[test]
    fn test_link_to_interface_info_fallback_name() {
        let link = LinkMessage::default();
        let info = NetlinkMonitor::link_to_interface_info(&link);
        assert!(info.name.starts_with("if"));
    }

    #[test]
    fn test_extract_link_snapshot_up_with_running() {
        let mut link = LinkMessage::default();
        link.header.flags.push(LinkFlag::Up);
        link.header.flags.push(LinkFlag::Running);
        link.attributes
            .push(LinkAttribute::IfName("eth0".to_string()));

        let state = NetlinkMonitor::extract_link_snapshot(&link);
        assert!(state.is_up);
        assert_eq!(state.name, "eth0");
    }

    #[test]
    fn test_extract_link_snapshot_down() {
        let mut link = LinkMessage::default();
        link.attributes
            .push(LinkAttribute::IfName("eth0".to_string()));

        let state = NetlinkMonitor::extract_link_snapshot(&link);
        assert!(!state.is_up);
    }

    #[test]
    fn test_extract_link_snapshot_up_without_running() {
        let mut link = LinkMessage::default();
        link.header.flags.push(LinkFlag::Up);
        link.attributes
            .push(LinkAttribute::IfName("eth0".to_string()));

        let state = NetlinkMonitor::extract_link_snapshot(&link);
        assert!(!state.is_up); // Requires both Up AND Running
    }

    #[tokio::test]
    async fn test_poll_changes_returns_empty_after_init() {
        let mut monitor = NetlinkMonitor::new().await.unwrap();
        let events = monitor.poll_changes().await.unwrap();
        // No changes since we just took the initial snapshot
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_get_active_interface_names() {
        let monitor = NetlinkMonitor::new().await.unwrap();
        let result = monitor.get_active_interface_names().await;
        assert!(result.is_ok());
    }
}
