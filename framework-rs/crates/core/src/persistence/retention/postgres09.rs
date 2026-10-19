//! Opt-in SQL adapter for application-owned retention tables and policies.
use crate::persistence::postgres09::begin_tenant_transaction;
use sqlx09 as sqlx;
include!("postgres_shared.rs");

// Only constructor-validated, quoted SQL identifiers are interpolated; data stays bound.
fn validated_identifiers_sql(sql: &str) -> sqlx::AssertSqlSafe<&str> {
    sqlx::AssertSqlSafe(sql)
}
