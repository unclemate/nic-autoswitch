//! nic-autoswitch - Linux network auto-switching daemon
//!
//! A daemon for automatically managing traffic routing based on
//! network interface status.
//!
//! # Modules
//!
//! - [`config`] - Configuration management
//! - [`error`] - Error types
//! - [`daemon`] - Daemon service
//! - [`monitor`] - Network monitoring
//! - [`router`] - Route management
//! - [`engine`] - Core engine
//! - `cli` - CLI tools (TODO)

pub mod config;
pub mod daemon;
pub mod engine;
pub mod error;
pub mod monitor;
pub mod router;

// TODO: Add these modules as they are implemented
// pub mod cli;

pub use config::Config;
pub use engine::{ActiveRoute, Destination, EventDispatcher, RuleExecutor, RuleMatcher};
pub use error::{NicAutoSwitchError, Result};
pub use monitor::{NetworkEvent, NetworkState, SharedNetworkState};
pub use router::{DnsResolver, RouteManager, RuleOperator};
