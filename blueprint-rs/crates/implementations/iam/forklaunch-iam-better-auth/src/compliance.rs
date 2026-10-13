//! Configured Account/Jwks PCI compliance. User and session fields are classified
//! `none` in this preset. Classified export values retain an explicit, audited
//! plaintext boundary and serialize as opaque objects like TS `CompliantField`.
use crate::store::{AuthStore, AuthTransaction, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use zeroize::Zeroizing;

pub struct ComplianceAccess<'a> {
    pub field: &'a str,
    pub level: &'static str,
}
/// No Debug or implicit plaintext serialization.
pub struct ProtectedField {
    value: Zeroizing<String>,
    path: String,
}
impl ProtectedField {
    /// Explicit plaintext access requires the caller's trusted audit sink.
    pub fn deanon(&self, audit: impl FnOnce(ComplianceAccess<'_>)) -> &str {
        audit(ComplianceAccess {
            field: &self.path,
            level: "pci",
        });
        &self.value
    }
}
impl Serialize for ProtectedField {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_map(Some(0))?.end()
    }
}
#[derive(Serialize)]
pub struct ComplianceRecord {
    pub id: String,
    #[serde(flatten)]
    fields: BTreeMap<String, Option<ProtectedField>>,
}
impl ComplianceRecord {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&ProtectedField> {
        self.fields.get(name).and_then(Option::as_ref)
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub user_id: String,
    pub entities: BTreeMap<String, Vec<ComplianceRecord>>,
    pub failures: Vec<ComplianceFailure>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EraseResult {
    pub entities_affected: Vec<String>,
    pub records_deleted: usize,
    pub failures: Vec<ComplianceFailure>,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComplianceFailure {
    pub entity_name: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}
pub struct ComplianceService<S> {
    store: Arc<S>,
    tenant_label: Option<String>,
}
impl<S: AuthStore> ComplianceService<S> {
    #[must_use]
    pub fn new(store: Arc<S>) -> Self {
        Self {
            store,
            tenant_label: None,
        }
    }
    /// Trusted report label; storage remains bound to the supplied store.
    #[must_use]
    pub fn with_tenant_label(mut self, label: String) -> Self {
        self.tenant_label = Some(label);
        self
    }
    /// Per-entity read failures are reported alongside available records.
    /// # Errors
    /// Reserved for unrecoverable operation-level failures.
    pub async fn export(&self, user_id: &str) -> BlueprintResult<ExportResult> {
        let mut result = ExportResult {
            user_id: user_id.to_owned(),
            entities: BTreeMap::new(),
            failures: vec![],
        };
        for (model, name, fields) in MODELS {
            let records = match self.read(*model, user_id).await {
                Ok(rows) => rows
                    .into_iter()
                    .map(|row| record(&row, name, fields))
                    .collect::<BlueprintResult<Vec<_>>>(),
                Err(error) => Err(error),
            };
            match records {
                Ok(records) => {
                    if !records.is_empty() {
                        result.entities.insert((*name).to_owned(), records);
                    }
                }
                Err(error) => result.failures.push(self.failure(name, &error)),
            }
        }
        Ok(result)
    }
    /// Reports unreadable entities, then atomically deletes readable owned rows.
    /// # Errors
    /// Rolls all pending deletions back on a write/commit failure.
    pub async fn erase(&self, user_id: &str) -> BlueprintResult<EraseResult> {
        let mut result = EraseResult {
            entities_affected: vec![],
            records_deleted: 0,
            failures: vec![],
        };
        let mut selected_rows = vec![];
        for (model, name, _) in MODELS {
            match self.read(*model, user_id).await {
                Ok(rows) => {
                    if !rows.is_empty() {
                        selected_rows.push((*model, *name, rows));
                    }
                }
                Err(error) => result.failures.push(self.failure(name, &error)),
            }
        }
        if selected_rows.is_empty() {
            return Ok(result);
        }
        let mut tx = self.store.begin(true).await?;
        for (model, name, rows) in selected_rows {
            let mut affected = false;
            for row in rows {
                let key = id(&row)?;
                if let Some(current) = tx.get(model, key).await?
                    && (model == Model::Jwks
                        || current.get("userId").and_then(Value::as_str) == Some(user_id))
                {
                    tx.delete(model, key).await?;
                    result.records_deleted += 1;
                    affected = true;
                }
            }
            if affected {
                result.entities_affected.push(name.to_owned());
            }
        }
        tx.commit().await?;
        Ok(result)
    }
    async fn read(&self, model: Model, user: &str) -> BlueprintResult<Vec<Value>> {
        let mut tx = self.store.begin(false).await?;
        let rows = selected(&mut tx, model, user).await?;
        tx.commit().await?;
        Ok(rows)
    }
    fn failure(&self, name: &str, error: &BlueprintError) -> ComplianceFailure {
        ComplianceFailure {
            entity_name: name.to_owned(),
            reason: error.to_string(),
            tenant_id: self.tenant_label.clone(),
        }
    }
}
// Matches the configured PostgreSQL metadata traversal order. Jwks has no user
// relation, so the generic TS walker falls back to its primary ID.
const MODELS: &[(Model, &str, &[&str])] = &[
    (Model::Jwks, "Jwks", &["privateKey"]),
    (
        Model::Account,
        "Account",
        &["accessToken", "refreshToken", "idToken", "password"],
    ),
];
async fn selected<T: AuthTransaction>(
    tx: &mut T,
    model: Model,
    user: &str,
) -> BlueprintResult<Vec<Value>> {
    if model == Model::Jwks {
        return Ok(tx.get(model, user).await?.into_iter().collect());
    }
    Ok(crate::store::find_all(tx, model, "userId", user)
        .await?
        .into_iter()
        .filter(|row| row.get("userId").and_then(Value::as_str) == Some(user))
        .collect())
}
fn id(row: &Value) -> BlueprintResult<&str> {
    row.get("id").and_then(Value::as_str).ok_or_else(invalid)
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "better_auth_compliance_invalid",
        "Invalid classified authentication record",
    )
}
fn record(row: &Value, model: &str, fields: &[&str]) -> BlueprintResult<ComplianceRecord> {
    let fields = fields
        .iter()
        .map(|field| {
            let value = match row.get(*field) {
                None | Some(Value::Null) => None,
                Some(Value::String(value)) => Some(ProtectedField {
                    value: Zeroizing::new(value.clone()),
                    path: format!("{model}.{field}"),
                }),
                _ => return Err(invalid()),
            };
            Ok(((*field).to_owned(), value))
        })
        .collect::<BlueprintResult<_>>()?;
    Ok(ComplianceRecord {
        id: id(row)?.to_owned(),
        fields,
    })
}

#[cfg(test)]
pub(crate) mod tests;
