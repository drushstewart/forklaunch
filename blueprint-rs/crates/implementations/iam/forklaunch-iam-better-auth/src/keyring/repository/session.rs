use super::super::{
    AuthStore, AuthTransaction, BlueprintResult, DateTime, Utc,
    adapter::{JwksAdapter, JwksAdapterContext, JwksKeyMaterial, adapter_error},
};
use super::{KeyRecord, KeyRepository, NewKeyRecord};
/// Default operations retain one transaction; callbacks never run under its lock.
pub(in super::super) struct KeySession<'a, S: AuthStore> {
    store: &'a S,
    transaction: Option<S::Transaction<'a>>,
    write: bool,
    adapter: Option<&'a JwksAdapter>,
    base_url: &'a str,
    now: DateTime<Utc>,
}
impl<'a, S: AuthStore> KeySession<'a, S> {
    pub(in super::super) fn new(
        store: &'a S,
        write: bool,
        adapter: Option<&'a JwksAdapter>,
        base_url: &'a str,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            store,
            transaction: None,
            write,
            adapter,
            base_url,
            now,
        }
    }
    async fn transaction(&mut self) -> BlueprintResult<&mut S::Transaction<'a>> {
        if self.transaction.is_none() {
            self.transaction = Some(self.store.begin(self.write).await?);
        }
        self.transaction.as_mut().ok_or_else(adapter_error)
    }
    pub(in super::super) async fn commit(&mut self) -> BlueprintResult<()> {
        if let Some(transaction) = self.transaction.take() {
            transaction.commit().await?;
        }
        Ok(())
    }
    fn context(&self, adapter: &JwksAdapter) -> JwksAdapterContext {
        JwksAdapterContext {
            tenant: adapter.tenant.clone(),
            base_url: self.base_url.into(),
            now: self.now,
        }
    }
    pub(in super::super) async fn read(&mut self) -> BlueprintResult<Vec<KeyRecord>> {
        if let Some(adapter) = self.adapter
            && let Some(callback) = &adapter.read
        {
            self.commit().await?;
            let records = callback(self.context(adapter))
                .await
                .map_err(|_| adapter_error())?
                .unwrap_or_default();
            if records.len() > 4096 {
                return Err(adapter_error());
            }
            return records
                .into_iter()
                .map(|record| record.row().map(KeyRecord))
                .collect();
        }
        KeyRepository::new(self.transaction().await?)
            .read_default_window()
            .await
    }
    pub(in super::super) async fn by_id(&mut self, id: &str) -> BlueprintResult<Option<KeyRecord>> {
        if self.adapter.is_some_and(|adapter| adapter.read.is_some()) {
            return Ok(self
                .read()
                .await?
                .into_iter()
                .find(|record| record.has_id(id)));
        }
        KeyRepository::new(self.transaction().await?)
            .by_id(id)
            .await
    }
    pub(in super::super) async fn select(
        &mut self,
        primary: &str,
    ) -> BlueprintResult<Option<KeyRecord>> {
        let mut candidates = self.live_candidates().await?;
        if let Some(index) = candidates
            .iter()
            .position(|row| row.matches_algorithm(primary))
        {
            return Ok(Some(candidates.remove(index)));
        }
        // Better Auth asks the custom reader again for latest-any fallback; it
        // may return a different snapshot. Default-store reads remain atomic.
        if self.adapter.is_some_and(|adapter| adapter.read.is_some()) {
            candidates = self.live_candidates().await?;
        }
        Ok(candidates.into_iter().next())
    }
    async fn live_candidates(&mut self) -> BlueprintResult<Vec<KeyRecord>> {
        let mut candidates = Vec::new();
        for row in self.read().await? {
            if row.expires_at()?.is_none_or(|expiry| expiry > self.now) {
                candidates.push((row.created_at()?, row));
            }
        }
        candidates.sort_by_key(|(created, _)| std::cmp::Reverse(*created));
        Ok(candidates.into_iter().map(|(_, row)| row).collect())
    }
    pub(in super::super) async fn create(
        &mut self,
        record: NewKeyRecord,
    ) -> BlueprintResult<KeyRecord> {
        if let Some(adapter) = self.adapter
            && let Some(callback) = &adapter.create
        {
            self.commit().await?;
            let material = JwksKeyMaterial::from_row(&record.0)?;
            return callback(material, self.context(adapter))
                .await
                .map_err(|_| adapter_error())?
                .row()
                .map(KeyRecord);
        }
        KeyRepository::new(self.transaction().await?)
            .create(record)
            .await
    }
    pub(in super::super) async fn retire(&mut self) -> BlueprintResult<()> {
        if self.adapter.is_some_and(JwksAdapter::active) {
            return Err(adapter_error());
        }
        let now = self.now;
        KeyRepository::new(self.transaction().await?)
            .retire_live(now)
            .await
    }
}
