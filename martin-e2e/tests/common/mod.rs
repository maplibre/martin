//! Common utilities for Martin e2e tests
//!
//! This module provides shared functionality for all e2e tests including:
//! - Server lifecycle management
//! - Binary path resolution
//! - Test fixture helpers
//! - HTTP client utilities

pub mod binaries;
pub mod fixtures;
pub mod server;

pub use binaries::*;
pub use fixtures::*;
pub use server::*;
