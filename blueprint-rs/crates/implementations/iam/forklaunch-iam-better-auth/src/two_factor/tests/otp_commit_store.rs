//! Owned fixture failures; never used by production HTTP handlers.
use super::{AuthCursor, AuthPage, AuthStore, AuthTransaction, AuthWindow, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::sync::Arc;
pub struct FaultStore<S> {
    pub store: Arc<S>,
    pub read: Option<Model>,
    pub page: Option<Model>,
    pub delete: Option<Model>,
    pub insert: Option<Model>,
    pub begin: bool,
    pub commit: Arc<std::sync::atomic::AtomicBool>,
    pub advance_on_session: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
    pub advance_on_verification: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
    pub advance_on_commit: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
}
pub struct FaultTransaction<T> {
    inner: T,
    read: Option<Model>,
    page: Option<Model>,
    delete: Option<Model>,
    insert: Option<Model>,
    commit: bool,
    advance_on_session: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
    advance_on_verification: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
    advance_on_commit: Option<(Arc<std::sync::atomic::AtomicI64>, i64)>,
}
impl<S: AuthStore> AuthStore for FaultStore<S> {
    type Transaction<'a>
        = FaultTransaction<S::Transaction<'a>>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        if self.begin {
            return Err(read_failure());
        }
        Ok(FaultTransaction {
            inner: self.store.begin(write).await?,
            read: self.read,
            page: self.page,
            delete: self.delete,
            insert: self.insert,
            commit: self.commit.load(std::sync::atomic::Ordering::SeqCst),
            advance_on_session: self.advance_on_session.clone(),
            advance_on_verification: self.advance_on_verification.clone(),
            advance_on_commit: self.advance_on_commit.clone(),
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
    async fn list_page(
        &mut self,
        model: Model,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> BlueprintResult<AuthPage> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
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
        if self.read == Some(model) || self.page == Some(model) {
            return Err(read_failure());
        }
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
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner
            .find_window(model, field, value, offset, limit)
            .await
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        if self.read == Some(model) {
            return Err(read_failure());
        }
        self.inner.list(model).await
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        if self.insert == Some(model) {
            return Err(read_failure());
        }
        self.inner.insert(model, value).await?;
        if model == Model::Session
            && let Some((clock, expiry)) = &self.advance_on_session
        {
            clock.store(*expiry, std::sync::atomic::Ordering::SeqCst);
        }
        if model == Model::Verification
            && let Some((clock, expiry)) = &self.advance_on_verification
        {
            clock.store(*expiry, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.replace(model, value).await?;
        if model == Model::Verification
            && let Some((clock, expiry)) = &self.advance_on_verification
        {
            clock.store(*expiry, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        if self.delete == Some(model) {
            return Err(BlueprintError::new(
                "better_auth_store_failure",
                "owned observer write failure",
            ));
        }
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        if self.commit {
            return Err(read_failure());
        }
        self.inner.commit().await?;
        if let Some((clock, expiry)) = &self.advance_on_commit {
            clock.store(*expiry, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
}
fn read_failure() -> BlueprintError {
    BlueprintError::new("better_auth_store_failure", "owned observer read failure")
}
