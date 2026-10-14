use super::*;
use crate::store::{
    AuthCursor, AuthPage, AuthWindow,
    memory::{MemoryAuthStore, MemoryAuthTransaction},
};
use std::sync::Mutex;
type Trace = Arc<Mutex<Vec<(&'static str, Model, usize)>>>;
#[derive(Default)]
struct Store {
    inner: MemoryAuthStore,
    trace: Trace,
}
struct Transaction {
    inner: MemoryAuthTransaction,
    trace: Trace,
}
impl AuthStore for Store {
    type Transaction<'a> = Transaction;
    async fn begin(&self, write: bool) -> BlueprintResult<Transaction> {
        Ok(Transaction {
            inner: self.inner.begin(write).await?,
            trace: self.trace.clone(),
        })
    }
}
impl AuthTransaction for Transaction {
    async fn get(&mut self, model: Model, id: &str) -> BlueprintResult<Option<Value>> {
        self.trace.lock().unwrap().push(("get", model, 1));
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
        self.trace
            .lock()
            .unwrap()
            .push(("get_many", model, ids.len()));
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
        let page = self
            .inner
            .find_page(model, field, value, after, limit)
            .await?;
        self.trace
            .lock()
            .unwrap()
            .push(("find_page", model, page.records.len()));
        Ok(page)
    }
    async fn find_window(
        &mut self,
        model: Model,
        field: &str,
        value: &str,
        offset: usize,
        limit: usize,
    ) -> BlueprintResult<AuthWindow> {
        let window = self
            .inner
            .find_window(model, field, value, offset, limit)
            .await?;
        self.trace
            .lock()
            .unwrap()
            .push(("find_window", model, window.records.len()));
        Ok(window)
    }
    async fn list(&mut self, model: Model) -> BlueprintResult<Vec<Value>> {
        panic!("unbounded list must not be called: {model:?}")
    }
    async fn insert(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.insert(model, value).await
    }
    async fn replace(&mut self, model: Model, value: Value) -> BlueprintResult<()> {
        self.inner.replace(model, value).await
    }
    async fn delete(&mut self, model: Model, id: &str) -> BlueprintResult<()> {
        self.inner.delete(model, id).await
    }
    async fn commit(self) -> BlueprintResult<()> {
        self.inner.commit().await
    }
}

#[tokio::test]
async fn ordinary_member_pages_bound_record_hydration_above_scan_limit() {
    let store = Arc::new(Store::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    for index in 0..4200 {
        let user = format!("window-user-{index}");
        tx.insert(Model::User,json!({"id":user,"name":"Public","email":format!("{index}@window.test"),"privateField":"hidden"})).await.unwrap();
        tx.insert(Model::Member,json!({"id":format!("window-member-{index}"),"userId":user,"organizationId":"window-org","role":"member","createdAt":now})).await.unwrap();
    }
    tx.insert(Model::Member,json!({"id":"foreign-window","userId":"window-user-1","organizationId":"other-org","role":"owner"})).await.unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(store.clone())
        .create("window-user-0", "", "", now)
        .await
        .unwrap();
    let service = OrganizationService::new(store.clone());
    for (offset, limit) in [(0, 100), (4100, 25), (4200, 1)] {
        store.trace.lock().unwrap().clear();
        let query = if offset == 0 {
            json!({"organizationId":"window-org"})
        } else {
            json!({"organizationId":"window-org","offset":offset,"limit":limit})
        };
        let page = service
            .list_members(session["token"].as_str().unwrap(), false, &query, now)
            .await
            .unwrap();
        assert_eq!(page["total"], 4200);
        let length = if offset == 4200 { 0 } else { limit };
        assert_eq!(page["members"].as_array().unwrap().len(), length);
        if length > 0 {
            assert_eq!(page["members"][0]["id"], format!("window-member-{offset}"));
            assert!(page["members"][0]["user"].get("privateField").is_none());
        }
        let trace = store.trace.lock().unwrap();
        assert_eq!(
            trace
                .iter()
                .filter(|(method, model, _)| *method == "get" && *model == Model::User)
                .count(),
            2,
            "only session and actor single-user lookups"
        );
        assert_eq!(
            trace
                .iter()
                .filter(|(method, _, _)| *method == "get_many")
                .copied()
                .collect::<Vec<_>>(),
            vec![("get_many", Model::User, length)]
        );
        assert_eq!(
            trace
                .iter()
                .filter(|(method, _, _)| *method == "find_window")
                .copied()
                .collect::<Vec<_>>(),
            vec![("find_window", Model::Member, length)]
        );
        assert_eq!(
            trace
                .iter()
                .filter(|(method, _, _)| *method == "find_page")
                .map(|(_, _, count)| count)
                .sum::<usize>(),
            1,
            "only actor membership must use keyset lookup"
        );
    }
}
