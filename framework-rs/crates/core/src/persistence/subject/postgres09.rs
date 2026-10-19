//! Explicit database-only subject erasure. External objects, caches, queues and
//! backups require separate adapters/reconciliation; this adapter never certifies
//! their erasure. Retain scope authentication keys while operations remain
//! retryable; removing a key fails closed. Runtime roles require tenant RLS and
//! neither SUPERUSER nor BYPASSRLS.
use crate::persistence::postgres09::begin_tenant_transaction;
use sqlx09 as sqlx;
include!("postgres_shared.rs");

// Only constructor-validated, quoted SQL identifiers are interpolated; data stays bound.
fn validated_identifiers_sql(sql: &str) -> sqlx::AssertSqlSafe<&str> {
    sqlx::AssertSqlSafe(sql)
}
