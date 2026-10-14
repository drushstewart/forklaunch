use super::{BlueprintError, BlueprintResult, Value};
use serde_json::Map;
/// Trusted external-signer metadata, intentionally without Debug/Serialize.
/// Absence differs from an empty header object; explicit JSON nulls are preserved.
/// This bounded JSON subset excludes JavaScript undefined/prototypes. Hints are
/// forwarded without algorithm validation: the callback owns enforcement.
#[derive(Clone, Default)]
pub struct JwtSigningContext {
    header: Option<Map<String, Value>>,
    key_id: Option<String>,
    algorithm: Option<String>,
}
impl JwtSigningContext {
    /// # Errors
    /// Rejects hints over 512/128 bytes, headers over 16 KiB of raw text,
    /// 1024 values or 32 nested levels, or 64 KiB of encoded JSON.
    pub fn new(
        header: Option<Map<String, Value>>,
        key_id: Option<String>,
        algorithm: Option<String>,
    ) -> BlueprintResult<Self> {
        let invalid = || {
            BlueprintError::new(
                "better_auth_signing_context",
                "Invalid external signing context",
            )
        };
        if key_id.as_ref().is_some_and(|v| v.len() > 512)
            || algorithm.as_ref().is_some_and(|v| v.len() > 128)
        {
            return Err(invalid());
        }
        if let Some(header) = &header {
            if header.len() > 1024 {
                return Err(invalid());
            }
            let mut text = header
                .keys()
                .fold(0usize, |total, key| total.saturating_add(key.len()));
            let mut nodes = 0;
            let mut pending: Vec<_> = header.values().map(|v| (v, 0usize)).collect();
            if pending.len() > 1024 || text > 16384 {
                return Err(invalid());
            }
            while let Some((value, depth)) = pending.pop() {
                nodes += 1;
                if nodes > 1024 || depth > 32 {
                    return Err(invalid());
                }
                match value {
                    Value::String(value) => text = text.saturating_add(value.len()),
                    Value::Array(values) => {
                        if values.len() + pending.len() > 1024 {
                            return Err(invalid());
                        }
                        pending.extend(values.iter().map(|v| (v, depth + 1)));
                    }
                    Value::Object(values) => {
                        if values.len() + pending.len() > 1024 {
                            return Err(invalid());
                        }
                        for (key, value) in values {
                            text = text.saturating_add(key.len());
                            pending.push((value, depth + 1));
                        }
                    }
                    _ => {}
                }
                if text > 16384 {
                    return Err(invalid());
                }
            }
            if serde_json::to_vec(header).map_err(|_| invalid())?.len() > 65536 {
                return Err(invalid());
            }
        }
        Ok(Self {
            header,
            key_id,
            algorithm,
        })
    }
    #[must_use]
    pub fn header(&self) -> Option<&Map<String, Value>> {
        self.header.as_ref()
    }
    #[must_use]
    pub fn signing_key_id(&self) -> Option<&str> {
        self.key_id.as_deref()
    }
    #[must_use]
    pub fn signing_algorithm(&self) -> Option<&str> {
        self.algorithm.as_deref()
    }
}
