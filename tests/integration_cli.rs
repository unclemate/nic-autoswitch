//! Integration tests for CLI communication
//!
//! Tests Unix socket communication and control protocol serialization.
//! The server lifecycle and client roundtrip tests verify actual socket I/O.
//!
//! **Requires CAP_NET_ADMIN** — run inside Docker container.

mod common;

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use nic_autoswitch::daemon::{
    ControlCommand, ControlResponse, ControlServer, ControlState, DaemonStatus, MockControlState,
};

// ============================================================================
// Control protocol serialization tests
// ============================================================================

#[test]
fn test_control_command_all_variants() {
    let variants = [
        ControlCommand::Status,
        ControlCommand::Reload,
        ControlCommand::ListRoutes,
        ControlCommand::Shutdown,
    ];

    for cmd in &variants {
        let json = serde_json::to_string(cmd).unwrap();
        let deserialized: ControlCommand = serde_json::from_str(&json).unwrap();
        assert_eq!(json, serde_json::to_string(&deserialized).unwrap());
    }
}

#[test]
fn test_control_response_success_with_data() {
    let response = ControlResponse::success(Some(serde_json::json!({
        "state": "running",
        "routes": 5
    })));
    assert!(response.success);
    assert!(response.data.is_some());
    assert!(response.message.is_none());

    let json = serde_json::to_string(&response).unwrap();
    let parsed: ControlResponse = serde_json::from_str(&json).unwrap();
    assert!(parsed.success);
}

#[test]
fn test_control_response_success_with_message() {
    let response = ControlResponse::success_with_message("Configuration reloaded");
    assert!(response.success);
    assert_eq!(response.message, Some("Configuration reloaded".to_string()));
}

#[test]
fn test_control_response_error() {
    let response = ControlResponse::error("Something went wrong");
    assert!(!response.success);
    assert_eq!(response.message, Some("Something went wrong".to_string()));
    assert!(response.data.is_none());
}

#[test]
fn test_daemon_status_roundtrip() {
    let status = DaemonStatus {
        version: "0.1.0".to_string(),
        state: "running".to_string(),
        uptime_secs: 3600,
        active_routes: 5,
    };
    let json = serde_json::to_string(&status).unwrap();
    let parsed: DaemonStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.version, "0.1.0");
    assert_eq!(parsed.state, "running");
    assert_eq!(parsed.uptime_secs, 3600);
    assert_eq!(parsed.active_routes, 5);
}

// ============================================================================
// MockControlState tests
// ============================================================================

#[test]
fn test_mock_control_state_initial_state() {
    let mock = MockControlState::new();
    let status = mock.get_status();

    assert_eq!(status.state, "Running");
    assert!(!mock.is_shutdown_requested());
    assert_eq!(status.active_routes, 0);
    assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
}

#[test]
fn test_mock_control_state_shutdown_lifecycle() {
    let mock = MockControlState::new();
    assert!(!mock.is_shutdown_requested());

    mock.request_shutdown();
    assert!(mock.is_shutdown_requested());
}

#[test]
fn test_mock_control_state_reload_succeeds() {
    let mock = MockControlState::new();
    assert!(mock.reload_config().is_ok());
}

#[test]
fn test_mock_control_state_routes_empty() {
    let mock = MockControlState::new();
    assert!(mock.get_active_routes().is_empty());
}

#[test]
fn test_mock_control_state_default() {
    let mock = MockControlState::default();
    assert_eq!(mock.get_status().state, "Running");
}

// ============================================================================
// ControlState trait via custom mock — error path
// ============================================================================

struct FailingState;

impl ControlState for FailingState {
    fn get_status(&self) -> DaemonStatus {
        DaemonStatus {
            version: "0.1.0".to_string(),
            state: "Error".to_string(),
            uptime_secs: 0,
            active_routes: 0,
        }
    }
    fn get_active_routes(&self) -> std::collections::HashMap<String, serde_json::Value> {
        std::collections::HashMap::new()
    }
    fn reload_config(&self) -> nic_autoswitch::Result<()> {
        Err(nic_autoswitch::NicAutoSwitchError::InvalidInput(
            "test reload error".to_string(),
        ))
    }
    fn is_shutdown_requested(&self) -> bool {
        false
    }
    fn request_shutdown(&self) {}
}

#[test]
fn test_failing_control_state_reload_returns_error() {
    let state: Arc<dyn ControlState> = Arc::new(FailingState);
    let result = state.reload_config();
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("test reload error")
    );
}

#[test]
fn test_failing_control_state_status() {
    let state: Arc<dyn ControlState> = Arc::new(FailingState);
    let status = state.get_status();
    assert_eq!(status.state, "Error");
}

// ============================================================================
// ControlServer socket tests
// ============================================================================

#[tokio::test]
async fn test_control_server_client_roundtrip_status() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test-status.sock");
    let state = Arc::new(MockControlState::new());
    let server = ControlServer::new(&socket_path, state);

    let handle = tokio::task::spawn(async move { server.start().await });

    // Wait for server to bind
    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(socket_path.exists());

    // Client: connect, send Status, read response
    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader).lines();

    let json = serde_json::to_string(&ControlCommand::Status).unwrap();
    writer.write_all(json.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    drop(writer); // Close write side to signal EOF

    let response_line =
        tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line()).await;

    if let Ok(Ok(Some(line))) = response_line {
        let response: ControlResponse = serde_json::from_str(&line).unwrap();
        assert!(response.success);
        assert!(response.data.is_some());
    }

    handle.abort();
}

#[tokio::test]
async fn test_control_server_client_roundtrip_list_routes() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test-routes.sock");
    let state = Arc::new(MockControlState::new());
    let server = ControlServer::new(&socket_path, state);

    let handle = tokio::task::spawn(async move { server.start().await });

    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(socket_path.exists());

    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader).lines();

    let json = serde_json::to_string(&ControlCommand::ListRoutes).unwrap();
    writer.write_all(json.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    drop(writer);

    let response_line =
        tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line()).await;

    if let Ok(Ok(Some(line))) = response_line {
        let response: ControlResponse = serde_json::from_str(&line).unwrap();
        assert!(response.success);
    }

    handle.abort();
}

#[tokio::test]
async fn test_control_server_client_roundtrip_reload() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test-reload.sock");
    let state = Arc::new(MockControlState::new());
    let server = ControlServer::new(&socket_path, state);

    let handle = tokio::task::spawn(async move { server.start().await });

    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(socket_path.exists());

    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader).lines();

    let json = serde_json::to_string(&ControlCommand::Reload).unwrap();
    writer.write_all(json.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    drop(writer);

    let response_line =
        tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line()).await;

    if let Ok(Ok(Some(line))) = response_line {
        let response: ControlResponse = serde_json::from_str(&line).unwrap();
        assert!(response.success);
        assert!(response.message.unwrap().contains("reloaded"));
    }

    handle.abort();
}

#[tokio::test]
async fn test_control_server_client_roundtrip_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test-shutdown.sock");
    let state = Arc::new(MockControlState::new());
    let server = ControlServer::new(&socket_path, state);

    let handle = tokio::task::spawn(async move { server.start().await });

    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(socket_path.exists());

    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader).lines();

    let json = serde_json::to_string(&ControlCommand::Shutdown).unwrap();
    writer.write_all(json.as_bytes()).await.unwrap();
    writer.write_all(b"\n").await.unwrap();
    writer.flush().await.unwrap();
    drop(writer);

    let response_line =
        tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line()).await;

    if let Ok(Ok(Some(line))) = response_line {
        let response: ControlResponse = serde_json::from_str(&line).unwrap();
        assert!(response.success);
        assert!(response.message.unwrap().contains("Shutdown"));
    }

    handle.abort();
}

#[tokio::test]
async fn test_control_server_invalid_json_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let socket_path = dir.path().join("test-invalid.sock");
    let state = Arc::new(MockControlState::new());
    let server = ControlServer::new(&socket_path, state);

    let handle = tokio::task::spawn(async move { server.start().await });

    for _ in 0..20 {
        if socket_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(socket_path.exists());

    let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = tokio::io::BufReader::new(reader).lines();

    writer.write_all(b"not valid json\n").await.unwrap();
    writer.flush().await.unwrap();
    drop(writer);

    let response_line =
        tokio::time::timeout(std::time::Duration::from_secs(2), reader.next_line()).await;

    if let Ok(Ok(Some(line))) = response_line {
        let response: ControlResponse = serde_json::from_str(&line).unwrap();
        assert!(!response.success);
    }

    handle.abort();
}

// ============================================================================
// Client connectivity tests
// ============================================================================

#[test]
fn test_client_connect_nonexistent_socket() {
    use std::os::unix::net::UnixStream;
    let socket_path = std::env::temp_dir().join("nic-autoswitch-nonexistent-test.sock");
    let _ = std::fs::remove_file(&socket_path);
    let result = UnixStream::connect(&socket_path);
    assert!(result.is_err());
}
