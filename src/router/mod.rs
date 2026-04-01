//! Routing management module
//!
//! This module provides DNS resolution, route table management, and rule operations.

mod dns;
mod manager;
mod rules;

pub use dns::*;
pub use manager::*;
pub use rules::*;
