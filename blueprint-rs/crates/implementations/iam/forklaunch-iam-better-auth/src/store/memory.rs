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
    ordered: BTreeMap<(Model, u64), String>,
    lookups: BTreeMap<(Model, &'static str, String, u64), String>,
    unique_values: BTreeMap<(Model, &'static str, String), String>,
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
        for key in unique_keys(model, row) {
            if self
                .pending
                .unique_values
                .get(&key)
                .is_some_and(|owner| owner != id)
            {
                return Err(conflict());
            }
        }
        Ok(())
    }
    fn index_record(&mut self, model: Model, row: &Value) -> BlueprintResult<()> {
        let id = record_id(row)?;
        let sequence = self
            .pending
            .rows
            .get(&(model, id.to_owned()))
            .map_or(self.pending.sequence, |(sequence, _)| *sequence);
        if let Some((_, previous)) = self.pending.rows.get(&(model, id.to_owned())) {
            for key in lookup_keys(model, previous, sequence) {
                self.pending.lookups.remove(&key);
            }
            for key in unique_keys(model, previous) {
                self.pending.unique_values.remove(&key);
            }
        }
        for key in lookup_keys(model, row, sequence) {
            self.pending.lookups.insert(key, id.to_owned());
        }
        for key in unique_keys(model, row) {
            self.pending.unique_values.insert(key, id.to_owned());
        }
        Ok(())
    }
}
fn unique_keys(model: Model, row: &Value) -> Vec<(Model, &'static str, String)> {
    model
        .unique_fields()
        .iter()
        .filter_map(|field| {
            row.get(*field)
                .and_then(Value::as_str)
                .map(|value| (model, *field, value.to_owned()))
        })
        .collect()
}
fn lookup_keys(
    model: Model,
    row: &Value,
    sequence: u64,
) -> Vec<(Model, &'static str, String, u64)> {
    model
        .indexed_fields()
        .iter()
        .filter_map(|field| {
            row.get(*field)
                .and_then(Value::as_str)
                .map(|value| (model, *field, value.to_owned(), sequence))
        })
        .collect()
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
        let field = model
            .unique_fields()
            .iter()
            .copied()
            .find(|candidate| *candidate == field)
            .ok_or_else(invalid)?;
        Ok(self
            .pending
            .unique_values
            .get(&(model, field, value.to_owned()))
            .and_then(|id| self.pending.rows.get(&(model, id.clone())))
            .map(|(_, row)| row.clone()))
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
    async fn list_page(
        &mut self,
        model: Model,
        after: Option<super::AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<super::AuthPage> {
        use std::ops::Bound::{Excluded, Included};
        let after = super::page_start(model, after, limit)?;
        let rows = self
            .pending
            .ordered
            .range((Excluded((model, after)), Included((model, u64::MAX))))
            .take(limit + 1)
            .map(|((_, sequence), id)| {
                let (_, value) = self
                    .pending
                    .rows
                    .get(&(model, id.clone()))
                    .ok_or_else(storage_error)?;
                Ok((*sequence, value.clone()))
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::page(model, rows, limit))
    }
    async fn find_page(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        after: Option<super::AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<super::AuthPage> {
        use std::ops::Bound::{Excluded, Included};
        let after = super::page_start(model, after, limit)?;
        let field = model
            .indexed_fields()
            .iter()
            .copied()
            .find(|candidate| *candidate == field)
            .ok_or_else(invalid)?;
        let rows = self
            .pending
            .lookups
            .range((
                Excluded((model, field, value.to_owned(), after)),
                Included((model, field, value.to_owned(), u64::MAX)),
            ))
            .take(limit + 1)
            .map(|((_, _, _, sequence), id)| {
                let (_, row) = self
                    .pending
                    .rows
                    .get(&(model, id.clone()))
                    .ok_or_else(storage_error)?;
                Ok((*sequence, row.clone()))
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::page(model, rows, limit))
    }
    async fn find_window(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        offset: usize,
        limit: usize,
    ) -> BlueprintResult<super::AuthWindow> {
        use std::ops::Bound::{Excluded, Included};
        super::page_start(model, None, limit)?;
        i64::try_from(offset).map_err(|_| invalid())?;
        let field = model
            .indexed_fields()
            .iter()
            .copied()
            .find(|candidate| *candidate == field)
            .ok_or_else(invalid)?;
        let entries = self.pending.lookups.range((
            Excluded((model, field, value.to_owned(), 0)),
            Included((model, field, value.to_owned(), u64::MAX)),
        ));
        let total = entries.clone().count();
        let records = entries
            .skip(offset)
            .take(limit)
            .map(|(_, id)| {
                self.pending
                    .rows
                    .get(&(model, id.clone()))
                    .map(|(_, row)| row.clone())
                    .ok_or_else(storage_error)
            })
            .collect::<BlueprintResult<Vec<_>>>()?;
        Ok(super::AuthWindow { records, total })
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
        self.index_record(model, &value)?;
        self.pending
            .ordered
            .insert((model, self.pending.sequence), key.1.clone());
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
        self.index_record(model, &value)?;
        self.pending.rows.insert(key, (sequence, value));
        Ok(())
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.require_write()?;
        if let Some((sequence, row)) = self.pending.rows.remove(&(model, id.to_owned())) {
            self.pending.ordered.remove(&(model, sequence));
            for key in unique_keys(model, &row) {
                self.pending.unique_values.remove(&key);
            }
            for key in lookup_keys(model, &row, sequence) {
                self.pending.lookups.remove(&key);
            }
        }
        Ok(())
    }
    async fn commit(mut self) -> BlueprintResult<()> {
        *self.guard = std::mem::take(&mut self.pending);
        Ok(())
    }
}
