//! Transactional external-erasure work. Explicitly enumerate targets in the same
//! SQL transaction as source deletion. Completion covers only that sealed set;
//! adapters remain responsible for storage holds, ownership, versions and backups.
use super::postgres09::SubjectEraseAudit;
use crate::persistence::postgres09::begin_tenant_transaction;
use sqlx09 as sqlx;
include!("outbox_shared.rs");
