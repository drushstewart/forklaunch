//! Internal key persistence boundary. Only the encrypted tenant store is wired.
//! Separate read/create hooks must eventually share this boundary; no partial
//! publication-only customization is exposed.
use super::{
    AuthTransaction, BlueprintResult, DateTime, Model, SigningKey, Utc, Value, key_error,
    optional_time, public_key, required_time, restore,
};

/// Secret-bearing persisted material, intentionally without Debug/Serialize.
pub(super) struct KeyRecord(Value);
impl KeyRecord {
    pub(super) fn into_value(self) -> Value {
        self.0
    }
    pub(super) fn has_id(&self, id: &str) -> bool {
        self.0["id"] == id
    }
    pub(super) fn created_at(&self) -> BlueprintResult<DateTime<Utc>> {
        required_time(&self.0, "createdAt")
    }
    pub(super) fn expires_at(&self) -> BlueprintResult<Option<DateTime<Utc>>> {
        optional_time(&self.0, "expiresAt")
    }
    pub(super) fn matches_algorithm(&self, algorithm: &str) -> bool {
        self.0["alg"] == algorithm || self.0["alg"].is_null()
    }
    pub(super) fn signing_key(&self) -> BlueprintResult<SigningKey> {
        restore(&self.0)
    }
    pub(super) fn public_key(&self) -> BlueprintResult<Value> {
        public_key(&self.0)
    }
}
/// A creation record carries expiry before persistence, as upstream createJwk does.
/// Keeping creation distinct from a stored record makes its returned identity and
/// material authoritative when a complete custom repository is added later.
pub(super) struct NewKeyRecord(pub(super) Value);
pub(super) struct KeyRepository<'a, T> {
    transaction: &'a mut T,
}
impl<'a, T: AuthTransaction> KeyRepository<'a, T> {
    pub(super) fn new(transaction: &'a mut T) -> Self {
        Self { transaction }
    }
    pub(super) async fn read_default_window(&mut self) -> BlueprintResult<Vec<KeyRecord>> {
        Ok(self
            .transaction
            .list_page(Model::Jwks, None, 100)
            .await?
            .records
            .into_iter()
            .map(KeyRecord)
            .collect())
    }
    pub(super) async fn retire_live(&mut self, now: DateTime<Utc>) -> BlueprintResult<()> {
        let mut after = None;
        loop {
            let page = self.transaction.list_page(Model::Jwks, after, 256).await?;
            for mut row in page.records {
                if optional_time(&row, "expiresAt")?.is_none_or(|expires| expires > now) {
                    row["expiresAt"] = serde_json::json!(super::timestamp(now));
                    self.transaction.replace(Model::Jwks, row).await?;
                }
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        Ok(())
    }
    pub(super) async fn create(&mut self, record: NewKeyRecord) -> BlueprintResult<KeyRecord> {
        // AuthTransaction insert has no returned row; its contract persists this
        // exact document. The repository returns that document without another read.
        if !record.0.is_object() {
            return Err(key_error());
        }
        self.transaction
            .insert(Model::Jwks, record.0.clone())
            .await?;
        Ok(KeyRecord(record.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AuthStore, memory::MemoryAuthStore};
    use serde_json::json;
    #[tokio::test]
    async fn creation_returns_persisted_material_and_expiry_without_extra_mutation() {
        let store = MemoryAuthStore::default();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let expires = now + chrono::Duration::milliseconds(1500);
        let mut tx = store.begin(true).await.unwrap();
        let key = super::super::mint_config(&mut tx, now, "EdDSA", 2048, Some(expires))
            .await
            .unwrap();
        let rows = KeyRepository::new(&mut tx)
            .read_default_window()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].created_at().unwrap(), now);
        assert_eq!(rows[0].expires_at().unwrap(), Some(expires));
        assert_eq!(rows[0].public_key().unwrap(), key.public_jwk());
        assert_eq!(
            rows[0]
                .signing_key()
                .unwrap()
                .sign_payload(&json!({"sub":"owned"}))
                .unwrap(),
            key.sign_payload(&json!({"sub":"owned"})).unwrap()
        );
        tx.commit().await.unwrap();
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            KeyRepository::new(&mut tx)
                .read_default_window()
                .await
                .unwrap()[0]
                .expires_at()
                .unwrap(),
            Some(expires)
        );
        tx.commit().await.unwrap();
    }
    #[tokio::test]
    async fn bad_returned_material_rolls_back_and_readonly_creation_cannot_persist() {
        let store = MemoryAuthStore::default();
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        {
            let mut tx = store.begin(true).await.unwrap();
            let record = KeyRepository::new(&mut tx).create(NewKeyRecord(json!({"id":"bad","createdAt":super::super::timestamp(now),"privateKey":"invalid","publicKey":"{}"}))).await.unwrap();
            assert!(record.signing_key().is_err());
        }
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            KeyRepository::new(&mut tx)
                .read_default_window()
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            KeyRepository::new(&mut tx)
                .create(NewKeyRecord(json!({"id":"readonly"})))
                .await
                .is_err()
        );
        tx.commit().await.unwrap();
    }
    #[tokio::test]
    async fn signing_creates_complete_expiry_and_retains_key_on_later_claim_failure() {
        let store = std::sync::Arc::new(MemoryAuthStore::default());
        let ring = super::super::KeyRing::new(store.clone(), "https://owned.example.test");
        let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
        let input =
            json!({"payload":{"sub":42},"overrideOptions":{"jwks":{"rotationInterval":1.5}}});
        assert!(ring.sign_jwt(Some(&input), now).await.is_err());
        let mut tx = store.begin(false).await.unwrap();
        let records = KeyRepository::new(&mut tx)
            .read_default_window()
            .await
            .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].expires_at().unwrap(),
            Some(now + chrono::Duration::milliseconds(1500))
        );
        tx.commit().await.unwrap();
        let input = json!({"payload":{},"overrideOptions":{"jwks":{"rotationInterval":1.5}}});
        ring.sign_jwt(Some(&input), now + chrono::Duration::milliseconds(1500))
            .await
            .unwrap();
        let mut tx = store.begin(false).await.unwrap();
        assert_eq!(
            KeyRepository::new(&mut tx)
                .read_default_window()
                .await
                .unwrap()
                .len(),
            2
        );
        tx.commit().await.unwrap();
    }
}
