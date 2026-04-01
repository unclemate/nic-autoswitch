//! Error types for nic-autoswitch
//!
//! This module defines the error types used throughout the application.

use thiserror::Error;

/// Main error type for nic-autoswitch
#[derive(Debug, Error)]
pub enum NicAutoSwitchError {
    /// Configuration related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Network operation errors
    #[error("Network operation failed: {0}")]
    Network(String),

    /// DNS resolution errors
    #[error("DNS resolution failed: {0}")]
    Dns(String),

    /// Route operation errors
    #[error("Route operation failed: {0}")]
    Route(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// D-Bus communication errors
    #[error("D-Bus error: {0}")]
    Dbus(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Operation timeout
    #[error("Operation timed out")]
    Timeout,

    /// TOML parsing errors
    #[error("TOML parsing error: {0}")]
    Toml(String),

    /// JSON serialization/deserialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<toml::de::Error> for NicAutoSwitchError {
    fn from(e: toml::de::Error) -> Self {
        NicAutoSwitchError::Toml(e.to_string())
    }
}

/// Result type alias for nic-autoswitch
pub type Result<T> = std::result::Result<T, NicAutoSwitchError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_config() {
        let err = NicAutoSwitchError::Config("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_error_display_network() {
        let err = NicAutoSwitchError::Network("connection failed".to_string());
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: NicAutoSwitchError = io_err.into();
        assert!(matches!(err, NicAutoSwitchError::Io(_)));
    }

    #[test]
    fn test_error_from_toml() {
        let toml_err: toml::de::Error =
            toml::from_str::<serde_json::Value>("invalid [[[ toml").unwrap_err();
        let err: NicAutoSwitchError = toml_err.into();
        assert!(matches!(err, NicAutoSwitchError::Toml(_)));
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_error_from_json() {
        let json_err: serde_json::Error = serde_json::from_str::<String>("not json").unwrap_err();
        let err: NicAutoSwitchError = json_err.into();
        assert!(matches!(err, NicAutoSwitchError::Json(_)));
    }

    #[test]
    fn test_error_display_dns() {
        let err = NicAutoSwitchError::Dns("resolution timeout".to_string());
        assert!(err.to_string().contains("resolution timeout"));
    }

    #[test]
    fn test_error_display_route() {
        let err = NicAutoSwitchError::Route("table not found".to_string());
        assert!(err.to_string().contains("table not found"));
    }

    #[test]
    fn test_error_display_dbus() {
        let err = NicAutoSwitchError::Dbus("connection failed".to_string());
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn test_error_display_invalid_input() {
        let err = NicAutoSwitchError::InvalidInput("bad value".to_string());
        assert!(err.to_string().contains("bad value"));
    }

    #[test]
    fn test_error_display_timeout() {
        let err = NicAutoSwitchError::Timeout;
        assert!(err.to_string().contains("timed out"));
    }

    #[test]
    fn test_result_type_alias() {
        fn returns_ok() -> Result<String> {
            Ok("success".to_string())
        }
        fn returns_err() -> Result<String> {
            Err(NicAutoSwitchError::Timeout)
        }
        assert!(returns_ok().is_ok());
        assert!(returns_err().is_err());
    }
}
