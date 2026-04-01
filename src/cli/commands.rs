//! CLI command implementations
//!
//! This module provides the implementation for each CLI command.

use std::fmt::Display;

use serde::Deserialize;

use super::client::DaemonClient;
use nic_autoswitch::Result;

/// Command executor for CLI operations
pub struct CommandExecutor {
    client: DaemonClient,
}

impl CommandExecutor {
    /// Create a new command executor with default client
    pub fn new() -> Self {
        Self {
            client: DaemonClient::default(),
        }
    }

    /// Create a command executor with a custom socket path
    pub fn with_socket_path<P: AsRef<std::path::Path>>(socket_path: P) -> Self {
        Self {
            client: DaemonClient::new(socket_path),
        }
    }

    /// Execute the status command
    pub fn status(&self) -> Result<()> {
        let status = self.client.get_status()?;
        print_json_formatted(&status);
        Ok(())
    }

    /// Execute the routes command
    pub fn routes(&self) -> Result<()> {
        let routes = self.client.get_routes()?;
        if routes.is_null()
            || (routes.is_object() && routes.as_object().is_none_or(|o| o.is_empty()))
        {
            println!("No active routes");
        } else {
            print_json_formatted(&routes);
        }
        Ok(())
    }

    /// Execute the reload command
    pub fn reload(&self) -> Result<()> {
        let message = self.client.reload_config()?;
        println!("✓ {}", message);
        Ok(())
    }

    /// Execute the shutdown command
    pub fn shutdown(&self) -> Result<()> {
        let message = self.client.shutdown()?;
        println!("✓ {}", message);
        Ok(())
    }

    /// Check daemon status and report
    pub fn check_daemon(&self) -> Result<()> {
        if self.client.is_daemon_running() {
            println!("Daemon is running");
            if let Ok(status) = self.client.get_status() {
                if let Some(state) = status.get("state") {
                    println!("State: {}", state);
                }
                if let Some(version) = status.get("version") {
                    println!("Version: {}", version);
                }
            }
        } else {
            println!("Daemon is not running");
        }
        Ok(())
    }
}

impl Default for CommandExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Print a JSON value with nice formatting
fn print_json_formatted(value: &serde_json::Value) {
    if let Ok(formatted) = serde_json::to_string_pretty(value) {
        println!("{}", formatted);
    } else {
        println!("{}", value);
    }
}

/// Daemon status response (for parsing)
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Daemon version
    pub version: String,
    /// Current state
    pub state: String,
    /// Uptime in seconds
    pub uptime_secs: u64,
    /// Number of active routes
    pub active_routes: usize,
}

impl Display for StatusResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Version: {}", self.version)?;
        writeln!(f, "State: {}", self.state)?;
        writeln!(f, "Uptime: {}s", self.uptime_secs)?;
        write!(f, "Active routes: {}", self.active_routes)
    }
}

/// Route entry response
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct RouteEntry {
    /// Rule name
    pub rule_name: String,
    /// Interface used
    pub interface: String,
    /// Table ID
    pub table_id: u32,
}

impl Display for RouteEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:<30} via {:<10} (table {})",
            self.rule_name, self.interface, self.table_id
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_executor_new() {
        let executor = CommandExecutor::new();
        assert!(!executor.client.is_daemon_running());
    }

    #[test]
    fn test_command_executor_with_socket_path() {
        let executor = CommandExecutor::with_socket_path("/custom/socket.sock");
        assert_eq!(
            executor.client.socket_path.to_str(),
            Some("/custom/socket.sock")
        );
    }

    #[test]
    fn test_status_response_display() {
        let status = StatusResponse {
            version: "0.1.0".to_string(),
            state: "Running".to_string(),
            uptime_secs: 3600,
            active_routes: 5,
        };
        let output = format!("{}", status);
        assert!(output.contains("Version: 0.1.0"));
        assert!(output.contains("State: Running"));
        assert!(output.contains("Uptime: 3600s"));
        assert!(output.contains("Active routes: 5"));
    }

    #[test]
    fn test_route_entry_display() {
        let route = RouteEntry {
            rule_name: "corp-cidr".to_string(),
            interface: "eth0".to_string(),
            table_id: 100,
        };
        let output = format!("{}", route);
        assert!(output.contains("corp-cidr"));
        assert!(output.contains("eth0"));
        assert!(output.contains("100"));
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_print_json_formatted_outputs_pretty() {
        let value = serde_json::json!({"key": "value", "number": 42});
        // print_json_formatted writes to stdout; just ensure no panic
        print_json_formatted(&value);
    }

    #[test]
    fn test_print_json_formatted_null_value() {
        let value = serde_json::Value::Null;
        print_json_formatted(&value);
    }

    #[test]
    fn test_print_json_formatted_object() {
        let value = serde_json::json!({"state": "Running", "version": "0.1.0"});
        print_json_formatted(&value);
    }

    #[test]
    fn test_command_executor_default() {
        let executor = CommandExecutor::default();
        assert!(!executor.client.is_daemon_running());
    }

    #[test]
    fn test_status_command_fails_when_daemon_not_running() {
        let executor = CommandExecutor::with_socket_path("/nonexistent/socket.sock");
        let result = executor.status();
        assert!(result.is_err());
    }

    #[test]
    fn test_routes_command_fails_when_daemon_not_running() {
        let executor = CommandExecutor::with_socket_path("/nonexistent/socket.sock");
        let result = executor.routes();
        assert!(result.is_err());
    }

    #[test]
    fn test_reload_command_fails_when_daemon_not_running() {
        let executor = CommandExecutor::with_socket_path("/nonexistent/socket.sock");
        let result = executor.reload();
        assert!(result.is_err());
    }

    #[test]
    fn test_shutdown_command_fails_when_daemon_not_running() {
        let executor = CommandExecutor::with_socket_path("/nonexistent/socket.sock");
        let result = executor.shutdown();
        assert!(result.is_err());
    }

    #[test]
    fn test_check_daemon_not_running() {
        let executor = CommandExecutor::with_socket_path("/nonexistent/socket.sock");
        // check_daemon should succeed (just prints) even when daemon not running
        let result = executor.check_daemon();
        assert!(result.is_ok());
    }
}
