//! Atomic reference backend, one store instance per trusted tenant.
use super::{
    AuthStore, AuthTransaction, Model, conflict, invalid, not_found, record_id, storage_error,
    validate_record,
};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, OwnedMutexGuard};

#[derive(Clone, Default)]
struct State {
    rows: BTreeMap<(Model, String), (u64, Value)>,
    sequence: u64,
}
#[derive(Clone, Default)]
pub struct MemoryAuthStore {
    state: Arc<Mutex<State>>,
}
pub struct MemoryAuthTransaction {
    guard: OwnedMutexGuard<State>,
    pending: State,
    write: bool,
}

impl AuthStore for MemoryAuthStore {
    type Transaction<'a> = MemoryAuthTransaction;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        let guard = self.state.clone().lock_owned().await;
        let pending = guard.clone();
        Ok(MemoryAuthTransaction {
            guard,
            pending,
            write,
        })
    }
}
impl MemoryAuthTransaction {
    fn require_write(&self) -> BlueprintResult<()> {
        if self.write { Ok(()) } else { Err(invalid()) }
    }
    fn unique(&self, model: Model, row: &Value) -> BlueprintResult<()> {
        validate_record(model, row)?;
        let id = record_id(row)?;
        for field in model.unique_fields() {
            if let Some(candidate) = row.get(*field).filter(|value| !value.is_null())
                && self.pending.rows.iter().any(|((family, key), (_, value))| {
                    *family == model && key != id && value.get(*field) == Some(candidate)
                })
            {
                return Err(conflict());
            }
        }
        Ok(())
    }
}
impl AuthTransaction for MemoryAuthTransaction {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        Ok(self
            .pending
            .rows
            .get(&(model, id.to_owned()))
            .map(|(_, value)| value.clone()))
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        if !model.unique_fields().contains(&field) {
            return Err(invalid());
        }
        Ok(self
            .pending
            .rows
            .iter()
            .find_map(|((family, _), (_, row))| {
                (*family == model && row.get(field).and_then(Value::as_str) == Some(value))
                    .then(|| row.clone())
            }))
    }
    async fn get_many(
        &mut self,
        model: Model,
        ids: &[String],
        limit: usize,
    ) -> BlueprintResult<Vec<Value>> {
        super::validate_many(ids, limit)?;
        let ids: std::collections::BTreeSet<_> = ids.iter().collect();
        let mut rows: Vec<_> = ids
            .into_iter()
            .filter_map(|id| self.pending.rows.get(&(model, id.clone())))
            .collect();
        rows.sort_by_key(|(sequence, _)| *sequence);
        Ok(rows
            .into_iter()
            .take(limit)
            .map(|(_, row)| row.clone())
            .collect())
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        let mut rows: Vec<_> = self
            .pending
            .rows
            .iter()
            .filter(|((family, _), _)| *family == model)
            .map(|(_, (sequence, value))| (*sequence, value.clone()))
            .collect();
        if rows.len() > 4096 {
            return Err(invalid());
        }
        rows.sort_by_key(|(sequence, _)| *sequence);
        Ok(rows.into_iter().map(|(_, value)| value).collect())
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.require_write()?;
        let key = (model, record_id(&value)?.to_owned());
        if self.pending.rows.contains_key(&key) {
            return Err(conflict());
        }
        self.unique(model, &value)?;
        self.pending.sequence = self
            .pending
            .sequence
            .checked_add(1)
            .ok_or_else(storage_error)?;
        self.pending
            .rows
            .insert(key, (self.pending.sequence, value));
        Ok(())
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.require_write()?;
        let key = (model, record_id(&value)?.to_owned());
        let sequence = self.pending.rows.get(&key).ok_or_else(not_found)?.0;
        self.unique(model, &value)?;
        self.pending.rows.insert(key, (sequence, value));
        Ok(())
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.require_write()?;
        self.pending.rows.remove(&(model, id.to_owned()));
        Ok(())
    }
    async fn commit(mut self) -> BlueprintResult<()> {
        *self.guard = std::mem::take(&mut self.pending);
        Ok(())
    }
}
