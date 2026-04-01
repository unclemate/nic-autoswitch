//! CLI tool for nic-autoswitch daemon
//!
//! Provides commands to interact with the running daemon via Unix socket.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod client;
mod commands;

use commands::CommandExecutor;

/// CLI tool for nic-autoswitch daemon
#[derive(Parser)]
#[command(name = "nic-autoswitch-cli")]
#[command(about = "CLI tool for nic-autoswitch daemon", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to the control socket
    #[arg(
        short,
        long,
        global = true,
        default_value = "/run/nic-autoswitch/control.sock"
    )]
    socket: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

/// Available commands
#[derive(Subcommand)]
enum Commands {
    /// Show daemon status and network information
    Status,

    /// Show active routing rules managed by the daemon
    Routes,

    /// Reload daemon configuration
    Reload,

    /// Request daemon shutdown
    Shutdown,

    /// Check if daemon is running
    Ping,
}

fn main() {
    let cli = Cli::parse();

    let executor = CommandExecutor::with_socket_path(&cli.socket);

    let result = match cli.command {
        Commands::Status => executor.status(),
        Commands::Routes => executor.routes(),
        Commands::Reload => executor.reload(),
        Commands::Shutdown => executor.shutdown(),
        Commands::Ping => executor.check_daemon(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
