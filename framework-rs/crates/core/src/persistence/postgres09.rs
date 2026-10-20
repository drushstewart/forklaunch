//! `PostgreSQL` pool admission for native runtimes using `SQLx` 0.9 (including Apalis).
use sqlx09 as driver;
include!("postgres_pool.rs");
