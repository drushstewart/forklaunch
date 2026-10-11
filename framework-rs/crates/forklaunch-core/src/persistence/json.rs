//! Fallible JSON serialization with optional tenant-bound, TypeScript-compatible envelopes.
use super::{
    TenantContext,
    encryption::{FieldEncryptor, is_encrypted_ciphertext},
};
use crate::common::{FrameworkError, FrameworkResult};
use serde::{Serialize, de::DeserializeOwned};

/// Holds keys, but deliberately exposes neither them nor a `Debug` implementation.
pub struct JsonRecordCodec {
    encryptor: FieldEncryptor,
}

impl JsonRecordCodec {
    #[must_use]
    pub fn new(encryptor: FieldEncryptor) -> Self {
        Self { encryptor }
    }

    /// # Errors
    /// Rejects absent/invalid encryption configuration.
    pub fn from_env() -> FrameworkResult<Self> {
        Ok(Self::new(FieldEncryptor::from_env()?))
    }

    /// Encrypts when compliance context is supplied; otherwise writes ordinary JSON.
    /// # Errors
    /// Returns a safe error on serialization or encryption failure.
    pub fn encode<T: Serialize + ?Sized>(
        &self,
        value: &T,
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<Vec<u8>> {
        let json = serde_json::to_string(value).map_err(|_| json_error())?;
        match tenant {
            Some(tenant) => self
                .encryptor
                .encrypt(&json, tenant.id())
                .map(String::into_bytes),
            None => Ok(json.into_bytes()),
        }
    }

    /// Reads legacy plaintext JSON alongside authenticated envelopes.
    /// Encrypted records require tenant context, and failures never become cache misses.
    /// # Errors
    /// Rejects invalid UTF-8/JSON, missing context, wrong tenants/keys and tampered envelopes.
    pub fn decode<T: DeserializeOwned>(
        &self,
        bytes: &[u8],
        tenant: Option<&TenantContext>,
    ) -> FrameworkResult<T> {
        let text = std::str::from_utf8(bytes).map_err(|_| json_error())?;
        let plaintext;
        let json = if is_encrypted_ciphertext(text) {
            let tenant = tenant.ok_or_else(|| {
                FrameworkError::new(
                    "record_tenant_required",
                    "Encrypted record requires tenant context",
                )
            })?;
            plaintext = self.encryptor.decrypt(text, tenant.id())?;
            &plaintext
        } else {
            text
        };
        serde_json::from_str(json).map_err(|_| json_error())
    }
}

fn json_error() -> FrameworkError {
    FrameworkError::new(
        "record_json_invalid",
        "Unable to encode or decode JSON record",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn authenticated_records_require_context_and_support_rotation_and_plaintext_migration() {
        let old = JsonRecordCodec::new(FieldEncryptor::new("old-record-key").unwrap());
        let codec = JsonRecordCodec::new(
            FieldEncryptor::new("new-record-key")
                .unwrap()
                .with_previous_keys(&["old-record-key".into()])
                .unwrap(),
        );
        let tenant = TenantContext::new("tenant-a").unwrap();
        let wrong = TenantContext::new("tenant-b").unwrap();
        for value in [
            json!({"secret":"秘密🙂","empty":null}),
            json!(false),
            json!(0),
            json!([]),
            json!("v2:ordinary-json-string"),
        ] {
            let bytes = old.encode(&value, Some(&tenant)).unwrap();
            assert_eq!(codec.decode::<Value>(&bytes, Some(&tenant)).unwrap(), value);
            assert!(codec.decode::<Value>(&bytes, None).is_err());
            assert!(codec.decode::<Value>(&bytes, Some(&wrong)).is_err());
            assert_eq!(
                codec
                    .decode::<Value>(&codec.encode(&value, None).unwrap(), Some(&tenant))
                    .unwrap(),
                value
            );
            let mut corrupt = bytes;
            corrupt.push(b'!');
            assert!(codec.decode::<Value>(&corrupt, Some(&tenant)).is_err());
        }
        for bad in [b"\xff".as_slice(), b"v2:bad:bad:bad", b"{", b"v9:unknown"] {
            assert!(codec.decode::<Value>(bad, Some(&tenant)).is_err());
        }
    }
}
