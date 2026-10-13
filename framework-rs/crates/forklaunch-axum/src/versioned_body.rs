//! Typed access to the body already parsed for API version selection.
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use forklaunch_http::SelectedVersion;
use serde::de::DeserializeOwned;

/// Decode the selected version's parsed body without consuming the wire body.
///
/// Place this before a raw `Bytes` extractor to use both representations. This
/// extractor requires a matched version and a declared body. It does not rerun
/// parsers or schema validation. `T` must agree with the selected body schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedBody<T>(pub T);

impl<S, T> FromRequestParts<S> for VersionedBody<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Send,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let value = parts
            .extensions
            .get::<SelectedVersion>()
            .and_then(|version| version.parsed_body.as_deref())
            .ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "VersionedBody requires a selected version with a body",
            ))?;
        T::deserialize(value).map(Self).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "Selected version body does not match the handler type",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn missing_selection_and_incompatible_types_have_stable_rejections() {
        let (mut parts, ()) = axum::http::Request::new(()).into_parts();
        assert_eq!(
            VersionedBody::<String>::from_request_parts(&mut parts, &())
                .await
                .unwrap_err()
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        parts.extensions.insert(SelectedVersion {
            version: "v1".into(),
            matched_versions: vec!["v1".into()],
            parsed_body: Some(Arc::new(serde_json::json!({"private":"secret"}))),
        });
        let error = VersionedBody::<String>::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            (
                StatusCode::BAD_REQUEST,
                "Selected version body does not match the handler type"
            )
        );
        let VersionedBody(value) =
            VersionedBody::<serde_json::Value>::from_request_parts(&mut parts, &())
                .await
                .unwrap();
        assert_eq!(value, serde_json::json!({"private":"secret"}));
    }
}
