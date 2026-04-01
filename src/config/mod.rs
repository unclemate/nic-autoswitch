//! Configuration management module
//!
//! This module provides configuration loading, validation, and hot-reload
//! capabilities for nic-autoswitch.

mod loader;
mod schema;
mod watcher;

pub use loader::*;
pub use schema::*;
pub use watcher::*;
