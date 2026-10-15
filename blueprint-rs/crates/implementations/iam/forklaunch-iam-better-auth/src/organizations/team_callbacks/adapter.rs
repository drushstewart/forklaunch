//! Trusted tenant-fixed operations used by organization policy callbacks.
use crate::store::{AuthCursor, AuthPage, AuthStore, AuthTransaction, Model};
use forklaunch_blueprint_core::BlueprintResult;
use serde_json::Value;
use std::{future::Future, pin::Pin};
/// A borrowed operation; it cannot outlive the callback's adapter.
pub type OrganizationAdapterFuture<'a, T> =
    Pin<Box<dyn Future<Output = BlueprintResult<T>> + Send + 'a>>;
/// Trusted callback access to the caller's adapter. Acceptance borrows its
/// transaction; other routes commit each operation separately. There is no
/// transaction lifecycle escape. Captured independent stores are not enlisted.
/// All operations retain the configured store's tenant and storage validation.
pub trait OrganizationCallbackAdapter: Send {
    fn get<'a>(
        &'a mut self,
        model: Model,
        id: &'a str,
    ) -> OrganizationAdapterFuture<'a, Option<Value>>;
    fn find_page<'a>(
        &'a mut self,
        model: Model,
        field: &'a str,
        value: &'a str,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> OrganizationAdapterFuture<'a, AuthPage>;
    fn insert(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()>;
    fn replace(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()>;
    fn delete<'a>(&'a mut self, model: Model, id: &'a str) -> OrganizationAdapterFuture<'a, ()>;
}
pub(in crate::organizations) struct TransactionAdapter<'a, T>(pub &'a mut T);
impl<T: AuthTransaction> OrganizationCallbackAdapter for TransactionAdapter<'_, T> {
    fn get<'a>(
        &'a mut self,
        model: Model,
        id: &'a str,
    ) -> OrganizationAdapterFuture<'a, Option<Value>> {
        Box::pin(self.0.get(model, id))
    }
    fn find_page<'a>(
        &'a mut self,
        model: Model,
        field: &'a str,
        value: &'a str,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> OrganizationAdapterFuture<'a, AuthPage> {
        Box::pin(self.0.find_page(model, field, value, after, limit))
    }
    fn insert(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()> {
        Box::pin(self.0.insert(model, row))
    }
    fn replace(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()> {
        Box::pin(self.0.replace(model, row))
    }
    fn delete<'a>(&'a mut self, model: Model, id: &'a str) -> OrganizationAdapterFuture<'a, ()> {
        Box::pin(self.0.delete(model, id))
    }
}
pub(in crate::organizations) struct AutocommitAdapter<'a, S>(pub &'a S);

impl<S: AuthStore> OrganizationCallbackAdapter for AutocommitAdapter<'_, S> {
    fn get<'a>(
        &'a mut self,
        model: Model,
        id: &'a str,
    ) -> OrganizationAdapterFuture<'a, Option<Value>> {
        Box::pin(get(self.0.begin(false), model, id))
    }
    fn find_page<'a>(
        &'a mut self,
        model: Model,
        field: &'a str,
        value: &'a str,
        after: Option<AuthCursor>,
        limit: usize,
    ) -> OrganizationAdapterFuture<'a, AuthPage> {
        Box::pin(find(self.0.begin(false), model, field, value, after, limit))
    }
    fn insert(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()> {
        Box::pin(write(self.0.begin(true), model, row, false))
    }
    fn replace(&mut self, model: Model, row: Value) -> OrganizationAdapterFuture<'_, ()> {
        Box::pin(write(self.0.begin(true), model, row, true))
    }
    fn delete<'a>(&'a mut self, model: Model, id: &'a str) -> OrganizationAdapterFuture<'a, ()> {
        Box::pin(delete(self.0.begin(true), model, id))
    }
}
async fn get<T: AuthTransaction>(
    pending: impl Future<Output = BlueprintResult<T>> + Send,
    model: Model,
    id: &str,
) -> BlueprintResult<Option<Value>> {
    let mut tx = pending.await?;
    let out = tx.get(model, id).await?;
    tx.commit().await?;
    Ok(out)
}
async fn find<T: AuthTransaction>(
    pending: impl Future<Output = BlueprintResult<T>> + Send,
    model: Model,
    field: &str,
    value: &str,
    after: Option<AuthCursor>,
    limit: usize,
) -> BlueprintResult<AuthPage> {
    let mut tx = pending.await?;
    let out = tx.find_page(model, field, value, after, limit).await?;
    tx.commit().await?;
    Ok(out)
}
async fn write<T: AuthTransaction>(
    pending: impl Future<Output = BlueprintResult<T>> + Send,
    model: Model,
    row: Value,
    replace: bool,
) -> BlueprintResult<()> {
    let mut tx = pending.await?;
    if replace {
        tx.replace(model, row).await?;
    } else {
        tx.insert(model, row).await?;
    }
    tx.commit().await
}
async fn delete<T: AuthTransaction>(
    pending: impl Future<Output = BlueprintResult<T>> + Send,
    model: Model,
    id: &str,
) -> BlueprintResult<()> {
    let mut tx = pending.await?;
    tx.delete(model, id).await?;
    tx.commit().await
}
