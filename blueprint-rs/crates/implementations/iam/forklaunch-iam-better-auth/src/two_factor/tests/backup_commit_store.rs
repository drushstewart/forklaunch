//! Owned fixture failures; never used by production HTTP handlers.
use super::{AuthCursor, AuthPage, AuthStore, AuthTransaction, AuthWindow, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::sync::Arc;
pub struct FaultStore<S> {
    pub store: Arc<S>,
    pub commits: Arc<std::sync::atomic::AtomicUsize>,
    pub fail_commit: usize,
    pub advance_commit: usize,
    pub clock: Arc<std::sync::atomic::AtomicI64>,
    pub late: i64,
    pub advance_session: bool,
}
pub struct FaultTransaction<T> {
    inner: T,
    commits: Arc<std::sync::atomic::AtomicUsize>,
    fail_commit: usize,
    advance_commit: usize,
    clock: Arc<std::sync::atomic::AtomicI64>,
    late: i64,
    advance_session: bool,
}
impl<S: AuthStore> AuthStore for FaultStore<S> {
    type Transaction<'a>
        = FaultTransaction<S::Transaction<'a>>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        Ok(FaultTransaction {
            inner: self.store.begin(write).await?,
            commits: self.commits.clone(),
            fail_commit: self.fail_commit,
            advance_commit: self.advance_commit,
            clock: self.clock.clone(),
            late: self.late,
            advance_session: self.advance_session,
        })
    }
}
impl<T: AuthTransaction> AuthTransaction for FaultTransaction<T> {
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
    async fn insert(&mut self, model: Model, record: Value) -> BlueprintResult<()> {
        self.inner.insert(model, record).await?;
        if model == Model::Session && self.advance_session {
            self.clock
                .store(self.late, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    async fn replace(&mut self, model: Model, record: Value) -> BlueprintResult<()> {
        self.inner.replace(model, record).await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        let count = self
            .commits
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if count == self.fail_commit {
            return Err(read_failure());
        }
        self.inner.commit().await?;
        if count == self.advance_commit {
            self.clock
                .store(self.late, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
}
fn read_failure() -> BlueprintError {
    BlueprintError::new("better_auth_store_failure", "owned observer read failure")
}
