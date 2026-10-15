//! Proves factor-read-only verification without granting factor UPDATE.
use super::{AuthCursor, AuthPage, AuthStore, AuthTransaction, AuthWindow, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::sync::Arc;
pub struct FactorReadOnly<S> {
    pub store: Arc<S>,
}
pub struct FactorTransaction<T> {
    inner: T,
}
impl<S: AuthStore> AuthStore for FactorReadOnly<S> {
    type Transaction<'a>
        = FactorTransaction<S::Transaction<'a>>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        Ok(FactorTransaction {
            inner: self.store.begin(write).await?,
        })
    }
}
impl<T: AuthTransaction> AuthTransaction for FactorTransaction<T> {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        self.inner.get(model, id).await
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        self.inner.find_unique(model, field, value).await
    }
    async fn get_many(
        &mut self,
        model: Model,
        ids: &[String],
        limit: usize,
    ) -> BlueprintResult<Vec<Value>> {
        self.inner.get_many(model, ids, limit).await
    }
    async fn list_page(
        &mut self,
        model: Model,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<AuthPage> {
        self.inner.list_page(model, after, limit).await
    }
    async fn find_page(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<AuthPage> {
        self.inner
            .find_page(model, field, value, after, limit)
            .await
    }
    async fn find_window(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        offset: usize,
        limit: usize,
    ) -> BlueprintResult<AuthWindow> {
        self.inner
            .find_window(model, field, value, offset, limit)
            .await
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        self.inner.list(model).await
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.insert(model, value).await
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        if model == Model::TwoFactor {
            return Err(BlueprintError::new(
                "better_auth_store_failure",
                "owned factor update denied",
            ));
        }
        self.inner.replace(model, value).await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        self.inner.commit().await
    }
}
