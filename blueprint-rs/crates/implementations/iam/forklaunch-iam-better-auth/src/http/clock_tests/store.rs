use crate::store::{AuthCursor, AuthPage, AuthStore, AuthTransaction, AuthWindow, Model};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Event {
    Get(Model),
    Find(Model),
    Insert(Model),
    Replace(Model),
    Delete(Model),
}
#[derive(Clone)]
pub(super) struct Advance {
    pub clock: Arc<AtomicI64>,
    pub armed: Arc<Mutex<Option<(Event, i64, usize)>>>,
}
impl Advance {
    pub fn arm(&self, event: Event, time: i64) {
        self.arm_after(event, time, 0);
    }
    pub fn arm_after(&self, event: Event, time: i64, skip: usize) {
        *self.armed.lock().unwrap() = Some((event, time, skip));
    }
    fn apply(&self, event: Event) {
        let mut armed = self.armed.lock().unwrap();
        if armed.as_ref().is_some_and(|(e, _, _)| *e == event) {
            if let Some((_, _, skip)) = armed.as_mut()
                && *skip > 0
            {
                *skip -= 1;
                return;
            }
            let (_, time, _) = armed.take().unwrap();
            self.clock.store(time, Ordering::SeqCst);
        }
    }
}
pub(super) struct Store<S> {
    pub inner: Arc<S>,
    pub advance: Advance,
}
pub(super) struct Transaction<T> {
    inner: T,
    advance: Advance,
}
impl<S: AuthStore> AuthStore for Store<S> {
    type Transaction<'a>
        = Transaction<S::Transaction<'a>>
    where
        Self: 'a;
    async fn begin(&self, write: bool) -> BlueprintResult<Self::Transaction<'_>> {
        Ok(Transaction {
            inner: self.inner.begin(write).await?,
            advance: self.advance.clone(),
        })
    }
}
impl<T: AuthTransaction> AuthTransaction for Transaction<T> {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        let v = self.inner.get(model, id).await?;
        self.advance.apply(Event::Get(model));
        Ok(v)
    }
    async fn find_unique(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
    ) -> BlueprintResult<Option<Value>> {
        let v = self.inner.find_unique(model, field, value).await?;
        self.advance.apply(Event::Find(model));
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
        self.inner.insert(model, value).await?;
        self.advance.apply(Event::Insert(model));
        Ok(())
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.replace(model, value).await?;
        self.advance.apply(Event::Replace(model));
        Ok(())
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.inner.delete(model, id).await?;
        self.advance.apply(Event::Delete(model));
        Ok(())
    }
    async fn commit(self) -> BlueprintResult<()> {
        self.inner.commit().await
    }
}
