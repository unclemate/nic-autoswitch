//! systemd integration
//!
//! This module provides systemd notify and watchdog support.

use std::time::Duration;

use tracing::{debug, warn};

/// systemd notification socket path
const NOTIFY_SOCKET_ENV: &str = "NOTIFY_SOCKET";
/// systemd watchdog interval environment variable
const WATCHDOG_USEC_ENV: &str = "WATCHDOG_USEC";

/// systemd notification type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyState {
    /// Service startup is complete
    Ready,
    /// Service is reloading configuration
    Reloading,
    /// Service is stopping
    Stopping,
    /// Service status message
    Status(&'static str),
    /// Custom status
    Custom(&'static str),
}

impl NotifyState {
    /// Convert to systemd notification string
    fn as_str(&self) -> &'static str {
        match self {
            NotifyState::Ready => "READY=1",
            NotifyState::Reloading => "RELOADING=1",
            NotifyState::Stopping => "STOPPING=1",
            NotifyState::Status(msg) => msg,
            NotifyState::Custom(msg) => msg,
        }
    }
}

/// systemd notify interface
pub struct SystemdNotify {
    /// Whether systemd notify is available
    enabled: bool,
    /// Watchdog interval in milliseconds (0 if disabled)
    watchdog_interval_ms: u64,
}

impl SystemdNotify {
    /// Create a new systemd notify interface
    pub fn new() -> Self {
        let enabled = std::env::var(NOTIFY_SOCKET_ENV).is_ok();
        let watchdog_interval_ms = Self::get_watchdog_interval();

        debug!(
            "systemd notify: enabled={}, watchdog_interval={}ms",
            enabled, watchdog_interval_ms
        );

        Self {
            enabled,
            watchdog_interval_ms,
        }
    }

    /// Get watchdog interval from environment
    fn get_watchdog_interval() -> u64 {
        std::env::var(WATCHDOG_USEC_ENV)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(|usec| usec / 1000 * 3 / 4) // 75% of the interval
            .unwrap_or(0)
    }

    /// Check if systemd notify is available
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Check if watchdog is enabled
    pub fn is_watchdog_enabled(&self) -> bool {
        self.watchdog_interval_ms > 0
    }

    /// Get watchdog interval
    pub fn watchdog_interval(&self) -> Duration {
        Duration::from_millis(self.watchdog_interval_ms)
    }

    /// Send notification to systemd
    pub fn notify(&self, state: NotifyState) {
        if !self.enabled {
            return;
        }

        let msg = state.as_str();
        debug!("Sending systemd notification: {}", msg);

        if let Err(e) = self.send_notification_raw(msg) {
            warn!("Failed to send systemd notification: {}", e);
        }
    }

    /// Send raw notification via Unix socket
    fn send_notification_raw(&self, msg: &str) -> std::io::Result<()> {
        use std::env;
        use std::os::unix::net::UnixDatagram;

        let socket_path = env::var(NOTIFY_SOCKET_ENV).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "NOTIFY_SOCKET not set")
        })?;

        let socket = UnixDatagram::unbound()?;
        let msg_bytes = msg.as_bytes();

        socket.send_to(msg_bytes, socket_path)?;

        Ok(())
    }

    /// Send ready notification
    pub fn notify_ready(&self) {
        self.notify(NotifyState::Ready);
    }

    /// Send reloading notification
    pub fn notify_reloading(&self) {
        self.notify(NotifyState::Reloading);
    }

    /// Send stopping notification
    pub fn notify_stopping(&self) {
        self.notify(NotifyState::Stopping);
    }

    /// Send status message
    pub fn notify_status(&self, status: &'static str) {
        self.notify(NotifyState::Status(status));
    }

    /// Send watchdog keepalive
    pub fn watchdog_keepalive(&self) {
        if self.is_watchdog_enabled() {
            self.notify(NotifyState::Custom("WATCHDOG=1"));
        }
    }
}

impl Default for SystemdNotify {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notify_state_ready() {
        assert_eq!(NotifyState::Ready.as_str(), "READY=1");
    }

    #[test]
    fn test_notify_state_reloading() {
        assert_eq!(NotifyState::Reloading.as_str(), "RELOADING=1");
    }

    #[test]
    fn test_notify_state_stopping() {
        assert_eq!(NotifyState::Stopping.as_str(), "STOPPING=1");
    }

    #[test]
    fn test_notify_state_status() {
        assert!(
            NotifyState::Status("STATUS=Running")
                .as_str()
                .contains("STATUS=")
        );
    }

    #[test]
    fn test_notify_state_status_message() {
        assert_eq!(
            NotifyState::Status("STATUS=Healthy").as_str(),
            "STATUS=Healthy"
        );
    }

    #[test]
    fn test_notify_state_custom() {
        assert_eq!(NotifyState::Custom("CUSTOM=test").as_str(), "CUSTOM=test");
    }

    #[test]
    fn test_systemd_notify_new() {
        let notify = SystemdNotify::new();
        // Without NOTIFY_SOCKET set, it should be disabled
        // But we can't test the actual notification in unit tests
        assert!(!notify.is_enabled() || std::env::var(NOTIFY_SOCKET_ENV).is_ok());
    }

    #[test]
    fn test_systemd_notify_default() {
        let notify = SystemdNotify::default();
        assert!(!notify.is_enabled() || std::env::var(NOTIFY_SOCKET_ENV).is_ok());
    }

    #[test]
    fn test_watchdog_interval_without_env() {
        // Without WATCHDOG_USEC, interval should be 0
        let original = std::env::var(WATCHDOG_USEC_ENV);
        // SAFETY: This is a test and we restore the original value afterwards
        unsafe {
            std::env::remove_var(WATCHDOG_USEC_ENV);
        }

        let interval = SystemdNotify::get_watchdog_interval();
        assert_eq!(interval, 0);

        // Restore original value if it existed
        if let Ok(val) = original {
            // SAFETY: This is a test and we're restoring the original value
            unsafe {
                std::env::set_var(WATCHDOG_USEC_ENV, val);
            }
        }
    }

    #[test]
    fn test_is_watchdog_enabled() {
        let notify = SystemdNotify::new();
        assert_eq!(
            notify.is_watchdog_enabled(),
            notify.watchdog_interval_ms > 0
        );
    }

    #[test]
    fn test_watchdog_interval_duration() {
        let notify = SystemdNotify::new();
        let duration = notify.watchdog_interval();
        assert_eq!(duration, Duration::from_millis(notify.watchdog_interval_ms));
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_notify_disabled_does_not_send() {
        // Without NOTIFY_SOCKET set, notify() should be a no-op
        let original = std::env::var(NOTIFY_SOCKET_ENV);
        unsafe {
            std::env::remove_var(NOTIFY_SOCKET_ENV);
        }

        let notify = SystemdNotify::new();
        assert!(!notify.is_enabled());

        // These should not panic
        notify.notify_ready();
        notify.notify_reloading();
        notify.notify_stopping();
        notify.notify_status("test");
        notify.watchdog_keepalive();

        if let Ok(val) = original {
            unsafe {
                std::env::set_var(NOTIFY_SOCKET_ENV, val);
            }
        }
    }

    #[test]
    fn test_watchdog_keepalive_disabled_noop() {
        let original = std::env::var(WATCHDOG_USEC_ENV);
        unsafe {
            std::env::remove_var(WATCHDOG_USEC_ENV);
        }

        let notify = SystemdNotify::new();
        assert!(!notify.is_watchdog_enabled());

        // Should not panic when watchdog is disabled
        notify.watchdog_keepalive();

        if let Ok(val) = original {
            unsafe {
                std::env::set_var(WATCHDOG_USEC_ENV, val);
            }
        }
    }

    #[test]
    fn test_watchdog_interval_with_valid_value() {
        let original_watchdog = std::env::var(WATCHDOG_USEC_ENV);
        let original_notify = std::env::var(NOTIFY_SOCKET_ENV);

        unsafe {
            std::env::set_var(WATCHDOG_USEC_ENV, "10000000");
        } // 10 seconds in usec
        unsafe {
            std::env::remove_var(NOTIFY_SOCKET_ENV);
        }

        let notify = SystemdNotify::new();
        assert!(notify.is_watchdog_enabled());
        // 10000000 usec = 10000ms, 75% = 7500ms
        assert_eq!(notify.watchdog_interval(), Duration::from_millis(7500));

        // Restore
        unsafe {
            std::env::remove_var(WATCHDOG_USEC_ENV);
            if let Ok(val) = original_watchdog {
                std::env::set_var(WATCHDOG_USEC_ENV, val);
            }
            if let Ok(val) = original_notify {
                std::env::set_var(NOTIFY_SOCKET_ENV, val);
            }
        }
    }

    #[test]
    fn test_watchdog_interval_with_invalid_value() {
        let original = std::env::var(WATCHDOG_USEC_ENV);
        unsafe {
            std::env::set_var(WATCHDOG_USEC_ENV, "not_a_number");
        }

        let interval = SystemdNotify::get_watchdog_interval();
        assert_eq!(interval, 0);

        if let Ok(val) = original {
            unsafe {
                std::env::set_var(WATCHDOG_USEC_ENV, val);
            }
        } else {
            unsafe {
                std::env::remove_var(WATCHDOG_USEC_ENV);
            }
        }
    }

    #[test]
    fn test_send_notification_raw_no_socket_returns_error() {
        // Remove env var right before the call to minimize race window with parallel tests
        let original = std::env::var(NOTIFY_SOCKET_ENV);
        unsafe {
            std::env::remove_var(NOTIFY_SOCKET_ENV);
        }
        let notify = SystemdNotify::new();
        // Remove again in case a parallel test re-set it
        unsafe {
            std::env::remove_var(NOTIFY_SOCKET_ENV);
        }
        let result = notify.send_notification_raw("READY=1");
        assert!(result.is_err());

        if let Ok(val) = original {
            unsafe {
                std::env::set_var(NOTIFY_SOCKET_ENV, val);
            }
        }
    }

    #[test]
    #[ignore = "env var race under tarpaulin parallel execution"]
    fn test_notify_with_real_socket_and_watchdog() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("notify.sock");

        // Create a Unix datagram socket to receive notifications
        use std::os::unix::net::UnixDatagram;
        let receiver = UnixDatagram::bind(&socket_path).unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();

        let original_notify = std::env::var(NOTIFY_SOCKET_ENV);
        let original_watchdog = std::env::var(WATCHDOG_USEC_ENV);

        // Set env for enabled notify + watchdog
        unsafe {
            std::env::set_var(NOTIFY_SOCKET_ENV, socket_path.to_str().unwrap());
            std::env::set_var(WATCHDOG_USEC_ENV, "10000000");
        }

        let notify = SystemdNotify::new();
        assert!(notify.is_enabled());
        assert!(notify.is_watchdog_enabled());

        // Test notify_ready → sends READY=1
        notify.notify_ready();
        let mut buf = [0u8; 256];
        let (size, _) = receiver.recv_from(&mut buf).unwrap();
        let msg = std::str::from_utf8(&buf[..size]).unwrap();
        assert_eq!(msg, "READY=1");

        // Test watchdog_keepalive → sends WATCHDOG=1
        notify.watchdog_keepalive();
        let (size, _) = receiver.recv_from(&mut buf).unwrap();
        let msg = std::str::from_utf8(&buf[..size]).unwrap();
        assert_eq!(msg, "WATCHDOG=1");

        // Test notify_stopping → sends STOPPING=1
        notify.notify_stopping();
        let (size, _) = receiver.recv_from(&mut buf).unwrap();
        let msg = std::str::from_utf8(&buf[..size]).unwrap();
        assert_eq!(msg, "STOPPING=1");

        // Cleanup
        unsafe {
            std::env::remove_var(NOTIFY_SOCKET_ENV);
            std::env::remove_var(WATCHDOG_USEC_ENV);
            if let Ok(val) = original_notify {
                std::env::set_var(NOTIFY_SOCKET_ENV, val);
            }
            if let Ok(val) = original_watchdog {
                std::env::set_var(WATCHDOG_USEC_ENV, val);
            }
        }
    }
}
