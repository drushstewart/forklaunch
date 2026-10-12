//! Owned fixture failures; never used by production HTTP handlers.
use super::{AuthStore, AuthTransaction, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::sync::Arc;
pub struct FaultStore<S> {
    pub store: Arc<S>,
    pub read: Option<Model>,
    pub delete: Option<Model>,
}
pub struct FaultTransaction<T> {
    inner: T,
    read: Option<Model>,
    delete: Option<Model>,
}
impl<S: AuthStore> AuthStore for FaultStore<S> {
    type Transaction<'a>
        = FaultTransaction<S::Transaction<'a>>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        Ok(FaultTransaction {
            inner: self.store.begin(write).await?,
            read: self.read,
            delete: self.delete,
        })
    }
}
impl<T: AuthTransaction> AuthTransaction for FaultTransaction<T> {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner.get(model, id).await
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner.find_unique(model, field, value).await
    }
    async fn get_many(
        &mut self,
        model: Model,
        ids: &[String],
        limit: usize,
    ) -> BlueprintResult<Vec<Value>> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner.get_many(model, ids, limit).await
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner.list(model).await
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.insert(model, value).await
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.replace(model, value).await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        if self.delete == Some(model) {
            return Err(BlueprintError::new(
                "Error",
                "owned compliance write failure",
            ));
        }
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        self.inner.commit().await
    }
}
fn read_failure() -> BlueprintError {
    BlueprintError::new("Error", "owned compliance read failure")
}
