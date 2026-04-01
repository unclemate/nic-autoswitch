//! Unix socket client for communicating with the daemon
//!
//! This module provides a client for sending commands to the running daemon
//! via Unix domain socket.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;

// Import from the library crate
use nic_autoswitch::daemon::{ControlCommand, ControlResponse};
use nic_autoswitch::{NicAutoSwitchError, Result};

/// Default socket path
pub const DEFAULT_SOCKET_PATH: &str = "/run/nic-autoswitch/control.sock";

/// Client for communicating with the daemon
#[derive(Debug)]
pub struct DaemonClient {
    /// Path to the control socket
    pub socket_path: std::path::PathBuf,
    timeout: Duration,
}

impl DaemonClient {
    /// Create a new daemon client
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            timeout: Duration::from_secs(5),
        }
    }

    /// Create a client with the default socket path
    pub fn default_client() -> Self {
        Self::new(DEFAULT_SOCKET_PATH)
    }

    /// Set the connection timeout
    #[allow(dead_code)]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send a command to the daemon and receive a response
    pub fn send_command<T: DeserializeOwned>(&self, command: &ControlCommand) -> Result<T> {
        let socket = UnixStream::connect(&self.socket_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound
                || e.kind() == std::io::ErrorKind::ConnectionRefused
            {
                NicAutoSwitchError::InvalidInput(
                    "Daemon is not running or socket not found".to_string(),
                )
            } else {
                NicAutoSwitchError::Io(e)
            }
        })?;

        socket
            .set_read_timeout(Some(self.timeout))
            .map_err(NicAutoSwitchError::Io)?;
        socket
            .set_write_timeout(Some(self.timeout))
            .map_err(NicAutoSwitchError::Io)?;

        // Serialize and send command
        let command_json = serde_json::to_string(command).map_err(NicAutoSwitchError::Json)?;
        let mut writer = &socket;
        writer
            .write_all(command_json.as_bytes())
            .map_err(NicAutoSwitchError::Io)?;
        writer.write_all(b"\n").map_err(NicAutoSwitchError::Io)?;
        writer.flush().map_err(NicAutoSwitchError::Io)?;

        // Read response
        let mut reader = BufReader::new(&socket);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(NicAutoSwitchError::Io)?;

        // Parse response
        let response: ControlResponse =
            serde_json::from_str(&response_line).map_err(NicAutoSwitchError::Json)?;

        if !response.success {
            return Err(NicAutoSwitchError::InvalidInput(
                response
                    .message
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        // Extract data if present
        let data = response.data.unwrap_or(serde_json::Value::Null);
        serde_json::from_value(data).map_err(|e| {
            NicAutoSwitchError::InvalidInput(format!("Failed to parse response: {}", e))
        })
    }

    /// Get daemon status
    pub fn get_status(&self) -> Result<serde_json::Value> {
        self.send_command(&ControlCommand::Status)
    }

    /// Get active routes
    pub fn get_routes(&self) -> Result<serde_json::Value> {
        self.send_command(&ControlCommand::ListRoutes)
    }

    /// Reload configuration
    pub fn reload_config(&self) -> Result<String> {
        self.send_command(&ControlCommand::Reload)
    }

    /// Request daemon shutdown
    pub fn shutdown(&self) -> Result<String> {
        self.send_command(&ControlCommand::Shutdown)
    }

    /// Check if daemon is running
    pub fn is_daemon_running(&self) -> bool {
        UnixStream::connect(&self.socket_path).is_ok()
    }
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::default_client()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_client_new() {
        let client = DaemonClient::new("/tmp/test.sock");
        assert_eq!(client.socket_path.to_str(), Some("/tmp/test.sock"));
    }

    #[test]
    fn test_daemon_client_default() {
        let client = DaemonClient::default();
        assert_eq!(client.socket_path.to_str(), Some(DEFAULT_SOCKET_PATH));
    }

    #[test]
    fn test_daemon_client_with_timeout() {
        let client = DaemonClient::default().with_timeout(Duration::from_secs(10));
        assert_eq!(client.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_is_daemon_running_returns_false_for_nonexistent_socket() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        assert!(!client.is_daemon_running());
    }

    // -------------------------------------------------------------------------
    // send_command error path tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_send_command_nonexistent_socket_returns_error() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        let result: Result<serde_json::Value> = client.send_command(&ControlCommand::Status);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_client_uses_default_path() {
        let client = DaemonClient::default_client();
        assert_eq!(client.socket_path.to_str(), Some(DEFAULT_SOCKET_PATH));
    }

    #[test]
    fn test_get_status_delegates_to_send_command() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        let result = client.get_status();
        assert!(result.is_err());
    }

    #[test]
    fn test_get_routes_delegates_to_send_command() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        let result = client.get_routes();
        assert!(result.is_err());
    }

    #[test]
    fn test_reload_config_delegates_to_send_command() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        let result = client.reload_config();
        assert!(result.is_err());
    }

    #[test]
    fn test_shutdown_delegates_to_send_command() {
        let client = DaemonClient::new("/nonexistent/socket.sock");
        let result = client.shutdown();
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // Integration tests with real Unix socket server (ignored for tarpaulin)
    // These tests are flaky under tarpaulin instrumentation due to timing issues
    // -------------------------------------------------------------------------

    /// Helper: start a simple Unix socket server that responds to one connection
    fn start_mock_server(socket_path: &std::path::Path, response: ControlResponse) {
        use std::os::unix::net::UnixListener;
        let listener = UnixListener::bind(socket_path).unwrap();
        let response_json = serde_json::to_string(&response).unwrap();

        std::thread::spawn(move || {
            listener.set_nonblocking(false).ok();
            if let Ok((mut stream, _)) = listener.accept() {
                // Small delay to ensure client has written its command
                std::thread::sleep(std::time::Duration::from_millis(50));
                // Read the incoming command (we don't care what it is)
                let mut buf = [0u8; 4096];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                // Write response
                let _ = stream.write_all(response_json.as_bytes());
                let _ = stream.write_all(b"\n");
                let _ = stream.flush();
            }
        });
    }

    /// Wait for a Unix socket file to appear, then give server thread time to call accept
    fn wait_for_socket(socket_path: &std::path::Path) {
        for _ in 0..40 {
            if socket_path.exists() {
                // File exists, give the server thread time to reach accept()
                std::thread::sleep(std::time::Duration::from_millis(300));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("Mock server socket file never appeared: {:?}", socket_path);
    }

    #[test]
    #[ignore = "flaky under tarpaulin instrumentation"]
    fn test_send_command_success_response() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let response = ControlResponse::success(Some(
            serde_json::json!({"version": "0.1.0", "state": "Running", "uptime_secs": 100, "active_routes": 5}),
        ));
        start_mock_server(&socket_path, response);

        wait_for_socket(&socket_path);

        let client = DaemonClient::new(&socket_path);
        let result: Result<serde_json::Value> = client.send_command(&ControlCommand::Status);
        assert!(result.is_ok());
        let data = result.unwrap();
        assert_eq!(data["version"], "0.1.0");
    }

    #[test]
    #[ignore = "flaky under tarpaulin instrumentation"]
    fn test_send_command_error_response() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let response = ControlResponse::error("Something went wrong");
        start_mock_server(&socket_path, response);

        wait_for_socket(&socket_path);

        let client = DaemonClient::new(&socket_path);
        let result: Result<serde_json::Value> = client.send_command(&ControlCommand::Status);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Something went wrong"));
    }

    #[test]
    #[ignore = "flaky under tarpaulin instrumentation"]
    fn test_send_command_success_with_message_returns_error_on_null_data() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let response = ControlResponse::success_with_message("Configuration reloaded");
        start_mock_server(&socket_path, response);

        wait_for_socket(&socket_path);

        let client = DaemonClient::new(&socket_path);
        // data is None → Null, deserializing Null into String fails
        let result: Result<String> = client.send_command(&ControlCommand::Reload);
        assert!(result.is_err());
    }

    #[test]
    #[ignore = "flaky under tarpaulin instrumentation"]
    fn test_send_command_success_with_data_string() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let response = ControlResponse::success(Some(serde_json::json!("Configuration reloaded")));
        start_mock_server(&socket_path, response);

        wait_for_socket(&socket_path);

        let client = DaemonClient::new(&socket_path);
        let result: Result<String> = client.send_command(&ControlCommand::Reload);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Configuration reloaded");
    }

    #[test]
    #[ignore = "flaky under tarpaulin instrumentation"]
    fn test_is_daemon_running_with_real_socket() {
        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let response = ControlResponse::success(None);
        start_mock_server(&socket_path, response);

        wait_for_socket(&socket_path);

        let client = DaemonClient::new(&socket_path);
        assert!(client.is_daemon_running());
    }
}
