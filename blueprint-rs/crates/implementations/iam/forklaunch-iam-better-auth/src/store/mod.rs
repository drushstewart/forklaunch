//! Atomic persistence for the eleven configured Better Auth models.
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::future::Future;

pub mod memory;
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Model {
    User,
    Session,
    Account,
    Verification,
    Jwks,
    Organization,
    Member,
    Invitation,
    Team,
    TeamMember,
    OrganizationRole,
}
impl Model {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Session => "session",
            Self::Account => "account",
            Self::Verification => "verification",
            Self::Jwks => "jwks",
            Self::Organization => "organization",
            Self::Member => "member",
            Self::Invitation => "invitation",
            Self::Team => "team",
            Self::TeamMember => "teamMember",
            Self::OrganizationRole => "organizationRole",
        }
    }
    const fn unique_fields(self) -> &'static [&'static str] {
        match self {
            Self::User => &["email", "subscription"],
            Self::Organization => &["slug"],
            _ => &[],
        }
    }
}

/// A trusted-tenant store. Requests cannot select a tenant through record data.
pub trait AuthStore: Send + Sync {
    type Transaction<'a>: AuthTransaction
    where
        Self: 'a;
    fn begin(
        &self,
        write: bool,
    ) -> impl Future<Output = BlueprintResult<Self::Transaction<'_>>> + Send;
}

/// Opaque insertion-order continuation, scoped to a model. Use within the same
/// transaction for a stable snapshot. Never use a cursor as authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthCursor {
    model: Model,
    sequence: u64,
}
/// One bounded page; `next` is present only when another record exists.
pub struct AuthPage {
    pub records: Vec<Value>,
    pub next: Option<AuthCursor>,
}

impl std::fmt::Debug for AuthPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthPage")
            .field("record_count", &self.records.len())
            .field("next", &self.next)
            .finish()
    }
}

/// One serialized graph operation. Drop without commit rolls back all changes.
/// Model relationships/cascades are the auth service's responsibility.
pub trait AuthTransaction: Send {
    fn get(
        &mut self,
        model: Model,
        id: &str,
    ) -> impl Future<Output = BlueprintResult<Option<Value>>> + Send;
    /// Exact lookup on a model's declared unique string field. Native backends
    /// avoid model scans, including when the model exceeds the list bound.
    /// Unsupported fields fail explicitly; values retain case and whitespace.
    fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> impl Future<Output = BlueprintResult<Option<Value>>> + Send;
    /// Loads a bounded set of primary keys in record insertion order. Duplicate
    /// IDs are returned once; missing IDs are skipped. Inputs and output limit
    /// are bounded to 4096 while the underlying model can contain more rows.
    fn get_many(
        &mut self,
        model: Model,
        ids: &[String],
        limit: usize,
    ) -> impl Future<Output = BlueprintResult<Vec<Value>>> + Send;
    /// Indexed keyset pagination in insertion order. Limits are 1..=4096;
    /// cross-model cursors are rejected. The transaction's lock keeps pages
    /// stable against concurrent writers. Replacements retain their position.
    fn list_page(
        &mut self,
        model: Model,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> impl Future<Output = BlueprintResult<AuthPage>> + Send;
    /// Returns insertion order, bounded to 4096 records per model. Oversized
    /// scans fail explicitly instead of silently truncating authorization data.
    fn list(&mut self, model: Model) -> impl Future<Output = BlueprintResult<Vec<Value>>> + Send;
    fn insert(
        &mut self,
        model: Model,
        value: Value,
    ) -> impl Future<Output = BlueprintResult<()>> + Send;
    fn replace(
        &mut self,
        model: Model,
        value: Value,
    ) -> impl Future<Output = BlueprintResult<()>> + Send;
    fn delete(
        &mut self,
        model: Model,
        id: &str,
    ) -> impl Future<Output = BlueprintResult<()>> + Send;
    fn commit(self) -> impl Future<Output = BlueprintResult<()>> + Send;
}

fn invalid() -> BlueprintError {
    BlueprintError::new(
        "better_auth_store_invalid",
        "Invalid authentication storage input",
    )
}
fn conflict() -> BlueprintError {
    BlueprintError::new(
        "better_auth_store_conflict",
        "Authentication record conflicts with an existing record",
    )
}
fn not_found() -> BlueprintError {
    BlueprintError::new(
        "better_auth_store_not_found",
        "Authentication record not found",
    )
}
fn storage_error() -> BlueprintError {
    BlueprintError::new(
        "better_auth_storage_error",
        "Authentication storage operation failed",
    )
}
fn record_id(value: &Value) -> BlueprintResult<&str> {
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(invalid)
}

fn validate_record(model: Model, row: &Value) -> BlueprintResult<()> {
    record_id(row)?;
    for field in model.unique_fields() {
        if row
            .get(*field)
            .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn validate_many(ids: &[String], limit: usize) -> BlueprintResult<()> {
    if ids.len() > 4096 || limit > 4096 || ids.iter().any(|id| id.len() > 256) {
        return Err(invalid());
    }
    Ok(())
}

fn page_start(model: Model, after: Option<AuthCursor>, limit: usize) -> BlueprintResult<u64> {
    if !(1..=4096).contains(&limit) || after.is_some_and(|cursor| cursor.model != model) {
        return Err(invalid());
    }
    Ok(after.map_or(0, |cursor| cursor.sequence))
}
fn page(model: Model, mut rows: Vec<(u64, Value)>, limit: usize) -> AuthPage {
    let next = (rows.len() > limit).then(|| AuthCursor {
        model,
        sequence: rows[limit - 1].0,
    });
    rows.truncate(limit);
    AuthPage {
        records: rows.into_iter().map(|(_, row)| row).collect(),
        next,
    }
}
