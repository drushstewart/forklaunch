//! Multipart version matching over an already buffered body; no I/O is polled.
use super::{ContractValidationError, ContractValidationLocation, body_too_large, invalid_body};
use futures_util::FutureExt;
use serde_json::{Map, Value};

/// Shared limits for version matching and typed Axum multipart extraction.
/// Overwritten repeated parts still count toward every limit.
#[derive(Debug, Clone, Copy)]
pub struct MultipartLimits {
    pub parts: usize,
    pub field_bytes: usize,
    pub total_bytes: usize,
}
impl Default for MultipartLimits {
    fn default() -> Self {
        Self {
            parts: 128,
            field_bytes: 1024 * 1024,
            total_bytes: 2 * 1024 * 1024,
        }
    }
}

pub(super) fn parse(
    bytes: &[u8],
    content_type: &str,
    limits: MultipartLimits,
) -> Result<Value, ContractValidationError> {
    if limits.parts == 0 || limits.field_bytes == 0 || limits.total_bytes == 0 {
        return Err(ContractValidationError::new(
            ContractValidationLocation::BodyParserConfiguration,
            "invalid multipart limits",
        ));
    }
    let boundary = multer::parse_boundary(content_type)
        .map_err(|_| invalid_body("invalid multipart boundary"))?;
    // This stream only contains immediately ready, owned memory. Poll once;
    // never nest an executor or block a runtime thread waiting for I/O.
    let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(bytes.to_vec())]);
    async move {
        let mut multipart = multer::Multipart::new(stream, boundary);
        let mut fields = Map::new();
        let mut count = 0_usize;
        let mut total = 0_usize;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|_| invalid_body("invalid multipart body"))?
        {
            count += 1;
            if count > limits.parts {
                return Err(body_too_large("multipart body exceeds limit"));
            }
            let name = field
                .name()
                .ok_or_else(|| invalid_body("multipart field has no name"))?
                .to_owned();
            let file = field.file_name().is_some()
                || field.content_type().is_some_and(|mime| {
                    mime.essence_str()
                        .eq_ignore_ascii_case("application/octet-stream")
                });
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| invalid_body("invalid multipart body"))?
            {
                if chunk.len() > limits.field_bytes.saturating_sub(bytes.len())
                    || chunk.len() > limits.total_bytes.saturating_sub(total)
                {
                    return Err(body_too_large("multipart body exceeds limit"));
                }
                total += chunk.len();
                bytes.extend_from_slice(&chunk);
            }
            let value = if file {
                Value::Array(bytes.into_iter().map(Value::from).collect())
            } else {
                Value::String(
                    String::from_utf8(bytes)
                        .map_err(|_| invalid_body("invalid multipart UTF-8 field"))?,
                )
            };
            fields.insert(name, value);
        }
        Ok(Value::Object(fields))
    }
    .now_or_never()
    .unwrap_or_else(|| Err(invalid_body("multipart parser did not complete")))
}

#[cfg(test)]
mod tests {
    use super::*;
    const MIME: &str = "multipart/form-data; boundary=b";
    const BODY: &[u8] = b"--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\none\r\n--b\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\ntwo\r\n--b--\r\n";
    #[test]
    fn repeated_parts_count_towards_every_shared_limit() {
        let limits = MultipartLimits {
            parts: 2,
            field_bytes: 3,
            total_bytes: 6,
        };
        assert_eq!(
            parse(BODY, MIME, limits),
            Ok(serde_json::json!({"a":"two"}))
        );
        for limits in [
            MultipartLimits { parts: 1, ..limits },
            MultipartLimits {
                field_bytes: 2,
                ..limits
            },
            MultipartLimits {
                total_bytes: 5,
                ..limits
            },
            MultipartLimits { parts: 0, ..limits },
        ] {
            assert!(parse(BODY, MIME, limits).is_err());
        }
        let errors = [parse(BODY, MIME, MultipartLimits { parts: 0, ..limits }).unwrap_err()];
        assert_eq!(
            crate::validation_error_response(&errors, "fixture").status_code,
            500
        );
        let errors = [parse(BODY, MIME, MultipartLimits { parts: 1, ..limits }).unwrap_err()];
        assert_eq!(
            crate::validation_error_response(&errors, "fixture").status_code,
            413
        );
        assert!(parse(BODY, "multipart/form-data", limits).is_err());
        assert!(parse(&BODY[..BODY.len() - 10], MIME, limits).is_err());
    }
}
