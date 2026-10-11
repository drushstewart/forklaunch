//! Native protocols for the configured Better Auth module.
//!
//! These primitives are independent of IAM base's Argon2 credential profile.
//! HTTP endpoints and persistent session management are implemented separately.
#![forbid(unsafe_code)]

pub mod cookies;
pub mod credentials;
pub mod jwt;
pub mod store;
