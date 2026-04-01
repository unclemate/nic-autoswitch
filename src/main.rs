//! nic-autoswitch daemon entry point
//!
//! This is the main entry point for the nic-autoswitch network
//! auto-switching daemon.

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing::{debug, error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use nic_autoswitch::Result;
use nic_autoswitch::daemon::{DaemonService, ServiceConfig};

/// nic-autoswitch - Network interface auto-switching daemon
#[derive(Parser, Debug)]
#[command(name = "nic-autoswitch")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "/etc/nic-autoswitch/config.toml")]
    config: PathBuf,

    /// Don't actually modify routing tables (dry run)
    #[arg(long)]
    dry_run: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long)]
    log_level: Option<String>,

    /// Run in foreground (don't daemonize)
    #[arg(short, long)]
    foreground: bool,
}

fn main() -> ExitCode {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    if let Err(e) = init_logging(args.log_level.as_deref(), args.foreground) {
        eprintln!("Failed to initialize logging: {}", e);
        return ExitCode::FAILURE;
    }

    info!("Starting nic-autoswitch daemon");

    // Run the daemon
    match run_daemon(args) {
        Ok(()) => {
            info!("Daemon exited normally");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("Daemon exited with error: {}", e);
            ExitCode::FAILURE
        }
    }
}

/// Initialize the logging system
///
/// In foreground mode (`foreground = true`), logs are always written to stdout
/// so the user can see them on the terminal. Otherwise, journald is preferred
/// when available (e.g. running under systemd).
fn init_logging(level: Option<&str>, foreground: bool) -> Result<()> {
    // Determine log level
    let log_level = level.unwrap_or("info");

    // Build the filter
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!("nic_autoswitch={}", log_level))
    });

    // Build the subscriber
    let registry = tracing_subscriber::registry().with(filter);

    if foreground {
        // Foreground mode: always use stdout so the user sees output
        registry.with(tracing_subscriber::fmt::layer()).init();
        debug!("Logging to stdout (foreground mode)");
    } else if let Ok(journald) = tracing_journald::layer() {
        // Daemon mode: prefer journald when available
        registry.with(journald).init();
        debug!("Logging to journald");
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
        debug!("Logging to stdout");
    }

    Ok(())
}

/// Run the daemon main loop using DaemonService
fn run_daemon(args: Args) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| nic_autoswitch::NicAutoSwitchError::InvalidInput(e.to_string()))?;

    rt.block_on(async {
        let service_config = ServiceConfig {
            config_path: args.config,
            socket_path: PathBuf::from("/run/nic-autoswitch/control.sock"),
            enable_hot_reload: true,
            dry_run: args.dry_run,
        };

        info!("Creating daemon service");
        let service = DaemonService::new(service_config)?;

        info!("Initializing daemon (rtnetlink connection, netlink monitor)");
        service.init().await?;

        info!("Starting daemon main loop");
        service.run().await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_args_default_config_path() {
        let args = Args::try_parse_from(["nic-autoswitch"]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(
            args.config,
            PathBuf::from("/etc/nic-autoswitch/config.toml")
        );
    }

    #[test]
    fn test_args_custom_config_path() {
        let args = Args::try_parse_from(["nic-autoswitch", "-c", "/custom/config.toml"]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.config, PathBuf::from("/custom/config.toml"));
    }

    #[test]
    fn test_args_dry_run_flag() {
        let args = Args::try_parse_from(["nic-autoswitch", "--dry-run"]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert!(args.dry_run);
    }

    #[test]
    fn test_args_log_level() {
        let args = Args::try_parse_from(["nic-autoswitch", "-l", "debug"]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert_eq!(args.log_level, Some("debug".to_string()));
    }

    #[test]
    fn test_args_foreground_flag() {
        let args = Args::try_parse_from(["nic-autoswitch", "-f"]);
        assert!(args.is_ok());
        let args = args.unwrap();
        assert!(args.foreground);
    }

    // -------------------------------------------------------------------------
    // Regression tests: DaemonService initialization inside runtime
    // -------------------------------------------------------------------------

    /// Verifies that DaemonService can be created inside a tokio runtime.
    #[test]
    fn test_daemon_service_creation_inside_runtime() {
        let rt = tokio::runtime::Runtime::new().expect("failed to create runtime");

        let result = rt.block_on(async {
            let config_content = r#"
[global]
monitor_interval = 5

[interfaces.eth0]
interface_type = "lan"
match_by = { name = "eth0" }
"#;
            let dir = tempfile::tempdir().expect("failed to create temp dir");
            let config_path = dir.path().join("config.toml");
            std::fs::write(&config_path, config_content).expect("failed to write config");

            let service_config = ServiceConfig {
                config_path,
                socket_path: PathBuf::from("/tmp/test-nic-autoswitch.sock"),
                enable_hot_reload: false,
                dry_run: true,
            };

            let service = DaemonService::new(service_config)?;
            assert_eq!(
                service.state(),
                nic_autoswitch::daemon::ServiceState::Initializing
            );

            Ok::<(), nic_autoswitch::NicAutoSwitchError>(())
        });

        assert!(result.is_ok());
    }

    /// Verifies that runtime creation works in sync context.
    #[test]
    fn test_runtime_creation_in_sync_context_succeeds() {
        let rt = tokio::runtime::Runtime::new();
        assert!(rt.is_ok(), "Runtime::new() should succeed in sync context");
        drop(rt);
    }
}
