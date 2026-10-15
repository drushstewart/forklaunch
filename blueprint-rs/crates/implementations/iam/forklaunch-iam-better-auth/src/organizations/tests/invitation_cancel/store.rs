use crate::store::{AuthCursor, AuthPage, AuthStore, AuthTransaction, AuthWindow, Model};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::sync::{Arc, Mutex};
pub(crate) enum AfterCancel {
    Delete(Model, String),
    Replace(Model, Value),
    Pause(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    None,
    CancelCommit,
    Insert,
    ReplacementCommit,
    CancelAcknowledgement,
    ReplacementAcknowledgement,
}
pub(crate) struct Store<S> {
    pub inner: Arc<S>,
    pub after: Mutex<Option<AfterCancel>>,
    pub failure: Failure,
}
pub(crate) struct Transaction<'a, S: AuthStore> {
    inner: S::Transaction<'a>,
    store: &'a Store<S>,
    canceled: bool,
    inserted: bool,
}
impl<S: AuthStore + 'static> AuthStore for Store<S> {
    type Transaction<'a>
        = Transaction<'a, S>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        Ok(Transaction {
            inner: self.inner.begin(write).await?,
            store: self,
            canceled: false,
            inserted: false,
        })
    }
}
fn fault() -> BlueprintError {
    BlueprintError::new("better_auth_storage_error", "Owned failure")
}
impl<S: AuthStore + 'static> AuthTransaction for Transaction<'_, S> {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        let v = self.inner.get(model, id).await?;
        Ok(v)
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        let v = self.inner.find_unique(model, field, value).await?;
        Ok(v)
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
        if model == Model::Invitation {
            self.inserted = true;
            if self.store.failure == Failure::Insert {
                return Err(fault());
            }
        }
        self.inner.insert(model, value).await
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        if model == Model::Invitation && value["status"] == "canceled" {
            self.canceled = true;
        }
        self.inner.replace(model, value).await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        if (self.canceled && self.store.failure == Failure::CancelCommit)
            || (self.inserted && self.store.failure == Failure::ReplacementCommit)
        {
            return Err(fault());
        }
        self.inner.commit().await?;
        if (self.canceled && self.store.failure == Failure::CancelAcknowledgement)
            || (self.inserted && self.store.failure == Failure::ReplacementAcknowledgement)
        {
            return Err(fault());
        }
        if self.canceled {
            let action = self.store.after.lock().unwrap().take();
            if let Some(action) = action {
                match action {
                    AfterCancel::Pause(ready, resume) => {
                        ready.notify_one();
                        resume.notified().await;
                    }
                    AfterCancel::Delete(model, id) => {
                        let mut tx = self.store.inner.begin(true).await?;
                        tx.delete(model, &id).await?;
                        tx.commit().await?;
                    }
                    AfterCancel::Replace(model, row) => {
                        let mut tx = self.store.inner.begin(true).await?;
                        tx.replace(model, row).await?;
                        tx.commit().await?;
                    }
                }
            }
        }
        Ok(())
    }
}
