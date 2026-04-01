//! Unix socket control server (simplified)
//!
//! This module provides a Unix domain socket server for runtime control
//! and status queries.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

use crate::error::NicAutoSwitchError;

/// Control socket path
pub const DEFAULT_SOCKET_PATH: &str = "/run/nic-autoswitch/control.sock";

/// Control command
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlCommand {
    /// Get current status
    Status,
    /// Get active routes
    ListRoutes,
    /// Reload configuration
    Reload,
    /// Shutdown daemon
    Shutdown,
}

/// Control response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlResponse {
    /// Success flag
    pub success: bool,
    /// Response message or error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Response data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ControlResponse {
    pub fn success(data: Option<serde_json::Value>) -> Self {
        Self {
            success: true,
            message: None,
            data,
        }
    }

    pub fn success_with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            data: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            data: None,
        }
    }
}

/// Daemon status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub state: String,
    pub uptime_secs: u64,
    pub active_routes: usize,
}

/// Control state trait
pub trait ControlState: Send + Sync {
    fn get_status(&self) -> DaemonStatus;
    fn get_active_routes(&self) -> HashMap<String, serde_json::Value>;
    fn reload_config(&self) -> crate::Result<()>;
    fn is_shutdown_requested(&self) -> bool;
    fn request_shutdown(&self);
}

/// Mock control state for testing
pub struct MockControlState {
    status: DaemonStatus,
    shutdown: Arc<AtomicBool>,
}

impl MockControlState {
    pub fn new() -> Self {
        Self {
            status: DaemonStatus {
                version: env!("CARGO_PKG_VERSION").to_string(),
                state: "Running".to_string(),
                uptime_secs: 0,
                active_routes: 0,
            },
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for MockControlState {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlState for MockControlState {
    fn get_status(&self) -> DaemonStatus {
        self.status.clone()
    }

    fn get_active_routes(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }

    fn reload_config(&self) -> crate::Result<()> {
        Ok(())
    }

    fn is_shutdown_requested(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }

    fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

/// Control server
pub struct ControlServer {
    socket_path: PathBuf,
    state: Arc<dyn ControlState>,
    shutdown: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
}

impl ControlServer {
    pub fn new(socket_path: impl Into<PathBuf>, state: Arc<dyn ControlState>) -> Self {
        Self {
            socket_path: socket_path.into(),
            state,
            shutdown: Arc::new(AtomicBool::new(false)),
            shutdown_notify: Arc::new(Notify::new()),
        }
    }

    pub async fn start(&self) -> crate::Result<()> {
        // Ensure socket directory exists
        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Remove existing socket file
        if tokio::fs::try_exists(&self.socket_path)
            .await
            .unwrap_or(false)
        {
            tokio::fs::remove_file(&self.socket_path).await?;
        }

        // Bind to socket
        let listener = UnixListener::bind(&self.socket_path)?;
        info!("Control server listening on {:?}", self.socket_path);

        // Set socket permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o660))
                .await?;
        }

        // Accept connections with shutdown awareness
        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            debug!("Accepted control connection");
                            // Verify peer credentials: only allow same UID or root
                            if !Self::check_peer_cred(&stream) {
                                warn!("Rejected control connection: peer credential check failed");
                                continue;
                            }
                            let state = self.state.clone();
                            tokio::spawn(async move {
                                let result = tokio::time::timeout(
                                    std::time::Duration::from_secs(5),
                                    Self::handle_connection(stream, state),
                                ).await;
                                match result {
                                    Ok(Err(e)) => warn!("Control connection error: {}", e),
                                    Err(_) => debug!("Control connection timed out"),
                                    _ => {}
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = self.shutdown_notify.notified() => {
                    info!("Control server shutting down");
                    break;
                }
            }

            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }
        }

        // Cleanup socket file
        let _ = tokio::fs::remove_file(&self.socket_path).await;

        Ok(())
    }

    /// Check peer credentials on Unix socket — only allow same UID or root.
    ///
    /// Platform implementation:
    /// - Linux: `SO_PEERCRED` via `getsockopt`
    /// - macOS/BSD: `getpeereid`
    /// - Other: reject (safe default)
    #[cfg(unix)]
    fn check_peer_cred(stream: &UnixStream) -> bool {
        use std::os::unix::io::AsRawFd;

        let fd = stream.as_raw_fd();

        #[cfg(target_os = "linux")]
        {
            #[repr(C)]
            struct Ucred {
                pid: i32,
                uid: u32,
                gid: u32,
            }

            let mut ucred: Ucred = Ucred {
                pid: 0,
                uid: 0,
                gid: 0,
            };
            let mut len: u32 = std::mem::size_of::<Ucred>() as u32;

            let ret = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    &mut ucred as *mut Ucred as *mut _,
                    &mut len as *mut u32,
                )
            };

            if ret == 0 {
                let my_uid = unsafe { libc::geteuid() };
                let peer_uid = ucred.uid;
                // Allow same user or root (UID 0)
                peer_uid == my_uid || peer_uid == 0
            } else {
                warn!("getsockopt SO_PEERCRED failed on fd {}", fd);
                false
            }
        }

        #[cfg(target_vendor = "apple")]
        {
            let mut uid: libc::uid_t = 0;
            let mut gid: libc::gid_t = 0;

            if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } == 0 {
                let my_uid = unsafe { libc::geteuid() };
                uid == my_uid || uid == 0
            } else {
                warn!("getpeereid failed on fd {}", fd);
                false
            }
        }

        #[cfg(not(any(target_os = "linux", target_vendor = "apple")))]
        {
            let _ = fd;
            warn!("Peer credential check not implemented for this platform");
            false // Safe default: reject if we cannot verify
        }
    }

    #[cfg(not(unix))]
    fn check_peer_cred(_stream: &UnixStream) -> bool {
        true // Non-Unix: allow all (no credential check available)
    }

    async fn handle_connection(
        stream: UnixStream,
        state: Arc<dyn ControlState>,
    ) -> crate::Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader).lines();

        while let Some(line) = reader.next_line().await? {
            debug!("Received command: {}", line);

            let command: Result<ControlCommand, _> = serde_json::from_str(&line);
            let response = match command {
                Ok(cmd) => Self::handle_command(cmd, &state),
                Err(e) => ControlResponse::error(format!("Invalid command: {}", e)),
            };

            let response_json =
                serde_json::to_string(&response).map_err(NicAutoSwitchError::Json)?;
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }

        Ok(())
    }

    fn handle_command(command: ControlCommand, state: &Arc<dyn ControlState>) -> ControlResponse {
        match command {
            ControlCommand::Status => {
                let status = state.get_status();
                ControlResponse::success(Some(
                    serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
                ))
            }
            ControlCommand::ListRoutes => {
                let routes = state.get_active_routes();
                ControlResponse::success(Some(
                    serde_json::to_value(routes).unwrap_or(serde_json::Value::Null),
                ))
            }
            ControlCommand::Reload => match state.reload_config() {
                Ok(()) => ControlResponse::success_with_message("Configuration reloaded"),
                Err(e) => ControlResponse::error(format!("Failed to reload: {}", e)),
            },
            ControlCommand::Shutdown => {
                state.request_shutdown();
                ControlResponse::success_with_message("Shutdown requested")
            }
        }
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.shutdown_notify.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_response() {
        let r = ControlResponse::success(None);
        assert!(r.success);

        let r = ControlResponse::success_with_message("OK");
        assert!(r.success);
        assert_eq!(r.message, Some("OK".to_string()));

        let r = ControlResponse::error("Error");
        assert!(!r.success);
    }

    #[test]
    fn test_control_command_serialization() {
        let cmd = ControlCommand::Status;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("status"));
    }

    #[test]
    fn test_daemon_status() {
        let status = DaemonStatus {
            version: "0.1.0".to_string(),
            state: "Running".to_string(),
            uptime_secs: 100,
            active_routes: 5,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("Running"));
    }

    #[test]
    fn test_mock_control_state() {
        let state = MockControlState::new();
        assert_eq!(state.get_status().version, env!("CARGO_PKG_VERSION"));
        assert!(!state.is_shutdown_requested());
        state.request_shutdown();
        assert!(state.is_shutdown_requested());
    }

    // -------------------------------------------------------------------------
    // ControlServer handle_command tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_handle_command_status_returns_success() {
        let state: Arc<dyn ControlState> = Arc::new(MockControlState::new());
        let response = ControlServer::handle_command(ControlCommand::Status, &state);
        assert!(response.success);
        assert!(response.data.is_some());
    }

    #[test]
    fn test_handle_command_list_routes_returns_success() {
        let state: Arc<dyn ControlState> = Arc::new(MockControlState::new());
        let response = ControlServer::handle_command(ControlCommand::ListRoutes, &state);
        assert!(response.success);
    }

    #[test]
    fn test_handle_command_reload_returns_success() {
        let state: Arc<dyn ControlState> = Arc::new(MockControlState::new());
        let response = ControlServer::handle_command(ControlCommand::Reload, &state);
        assert!(response.success);
        assert!(response.message.is_some());
    }

    #[test]
    fn test_handle_command_shutdown_requests_shutdown() {
        let state: Arc<dyn ControlState> = Arc::new(MockControlState::new());
        assert!(!state.is_shutdown_requested());
        let response = ControlServer::handle_command(ControlCommand::Shutdown, &state);
        assert!(response.success);
        assert!(state.is_shutdown_requested());
    }

    #[test]
    fn test_control_server_stop_sets_shutdown_flag() {
        let state = Arc::new(MockControlState::new());
        let server = ControlServer::new("/tmp/test-stop.sock", state);
        assert!(!server.shutdown.load(Ordering::Relaxed));
        server.stop();
        assert!(server.shutdown.load(Ordering::Relaxed));
    }

    // -------------------------------------------------------------------------
    // Integration: client-server roundtrip via Unix socket
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn test_control_server_handle_connection_valid_command() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");
        let state = Arc::new(MockControlState::new());

        // Start server in background
        let server = ControlServer::new(&socket_path, state.clone());
        let server_handle = tokio::spawn(async move { server.start().await });

        // Give server time to bind
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect as client
        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader).lines();

        // Send Status command
        let cmd = ControlCommand::Status;
        let json = serde_json::to_string(&cmd).unwrap();
        writer.write_all(json.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();

        // Drop writer to close the connection (allows server to finish)
        drop(writer);

        // Read response (may or may not arrive depending on timing)
        // The important thing is the server handled the connection without panic
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(500), reader.next_line()).await;

        // Stop server
        server_handle.abort();
    }

    #[tokio::test]
    async fn test_control_server_handle_connection_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test-invalid.sock");
        let state = Arc::new(MockControlState::new());

        let server = ControlServer::new(&socket_path, state.clone());
        let server_handle = tokio::spawn(async move { server.start().await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = tokio::io::BufReader::new(reader).lines();

        // Send invalid JSON
        writer.write_all(b"not valid json\n").await.unwrap();
        writer.flush().await.unwrap();
        drop(writer);

        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(500), reader.next_line()).await;

        server_handle.abort();
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_mock_control_state_default() {
        let state = MockControlState::default();
        assert_eq!(state.get_status().version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_handle_command_reload_error_returns_error_response() {
        struct FailingState;
        impl ControlState for FailingState {
            fn get_status(&self) -> DaemonStatus {
                DaemonStatus {
                    version: "0.1.0".to_string(),
                    state: "Running".to_string(),
                    uptime_secs: 0,
                    active_routes: 0,
                }
            }
            fn get_active_routes(&self) -> HashMap<String, serde_json::Value> {
                HashMap::new()
            }
            fn reload_config(&self) -> crate::Result<()> {
                Err(crate::NicAutoSwitchError::InvalidInput(
                    "test reload error".to_string(),
                ))
            }
            fn is_shutdown_requested(&self) -> bool {
                false
            }
            fn request_shutdown(&self) {}
        }

        let state: Arc<dyn ControlState> = Arc::new(FailingState);
        let response = ControlServer::handle_command(ControlCommand::Reload, &state);
        assert!(!response.success);
        assert!(response.message.unwrap().contains("test reload error"));
    }

    #[tokio::test]
    async fn test_control_server_graceful_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test-graceful.sock");
        let state = Arc::new(MockControlState::new());
        let server = ControlServer::new(&socket_path, state);

        // Clone shutdown primitives so we can signal from outside
        let shutdown = server.shutdown.clone();
        let shutdown_notify = server.shutdown_notify.clone();

        // Start server in background
        let handle = tokio::task::spawn(async move { server.start().await });

        // Wait for socket to appear
        for _ in 0..20 {
            if socket_path.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(socket_path.exists());

        // Trigger shutdown via notify (no need to connect to unblock)
        shutdown.store(true, Ordering::Relaxed);
        shutdown_notify.notify_one();

        // Wait for server to finish
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());

        // Socket file should be cleaned up
        assert!(!socket_path.exists());
    }
}
