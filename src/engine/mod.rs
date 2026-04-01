//! Core engine module
//!
//! This module provides the core logic for rule matching and execution.

mod dispatcher;
mod executor;
mod matcher;

pub use dispatcher::*;
pub use executor::*;
pub use matcher::*;
