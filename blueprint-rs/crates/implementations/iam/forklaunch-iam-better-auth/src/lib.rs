//! Native protocols for the configured Better Auth module.
//!
//! These primitives are independent of IAM base's Argon2 credential profile.
//! HTTP endpoints and persistent session management are implemented separately.
#![forbid(unsafe_code)]

pub mod accounts;
pub mod background;
pub mod compliance;
pub mod cookies;
pub mod credentials;
pub mod jwt;
pub mod keyring;
pub mod relay;
pub mod retention;
pub mod sessions;
pub mod store;
pub mod surfacing;
pub mod two_factor;

pub mod oauth;
pub mod organizations;
pub mod origins;

#[cfg(feature = "http")]
pub mod http;

mod ecmascript;
mod error_origin;

mod time_source;

#[cfg(all(test, feature = "postgres"))]
mod restricted_postgres_test;
