//! JSON object operations with optional tenant-bound authenticated encryption.
use crate::{AsyncObjectStore, ObjectMetadata};
use forklaunch_core::{
    common::FrameworkResult,
    persistence::{TenantContext, json::JsonRecordCodec},
};
use serde::{Serialize, de::DeserializeOwned};

pub struct JsonObjectStore<S> {
    store: S,
    codec: JsonRecordCodec,
}

impl<S: AsyncObjectStore> JsonObjectStore<S> {
    #[must_use]
    pub fn new(store: S, codec: JsonRecordCodec) -> Self {
        Self { store, codec }
    }

    #[must_use]
    pub fn into_inner(self) -> S {
        self.store
    }

    /// Leaves file/binary operations on the underlying store untouched.
    /// # Errors
    /// Propagates serialization, encryption and backend errors.
    pub async fn put_object<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
        metadata: ObjectMetadata,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<()> {
        let body = self.codec.encode(value, tenant)?;
        self.store.put(key, body, metadata).await?;
        Ok(())
    }

    /// Returns typed JSON with original content type/tags; misses remain `None`.
    /// # Errors
    /// Propagates backend and decoding/authentication errors, including wrong tenants.
    pub async fn get_object<T: DeserializeOwned>(
        &self,
        key: &str,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<Option<(T, ObjectMetadata)>> {
        self.store
            .get(key)
            .await?
            .map(|object| Ok((self.codec.decode(&object.body, tenant)?, object.metadata)))
            .transpose()
    }
}
