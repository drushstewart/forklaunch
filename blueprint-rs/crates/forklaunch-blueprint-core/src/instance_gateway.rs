//! Instance-signed platform transport compatible with the TypeScript gateway.
//! Credentials belong to trusted server configuration. Vendor keys never enter
//! this transport. Redirects and automatic mutation retries remain disabled.
use crate::{
    BlueprintError, BlueprintResult,
    provider_http::{ProviderHttpClient, ProviderResponse},
};
use forklaunch_auth::request_hmac::{SignedBody, generate_hmac_auth_header};
use reqwest::{Method, RequestBuilder};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use zeroize::Zeroizing;
#[derive(Clone)]
pub struct InstanceGatewayClient {
    http: ProviderHttpClient,
    instance_id: Arc<String>,
    secret: Arc<Zeroizing<String>>,
    mount_prefix: Arc<String>,
}
impl std::fmt::Debug for InstanceGatewayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceGatewayClient")
            .field("http", &self.http)
            .finish_non_exhaustive()
    }
}
impl InstanceGatewayClient {
    /// # Errors
    /// Rejects invalid credentials, remote plaintext and unsafe gateway URLs.
    pub fn new(gateway_url: &str, instance_id: String, hmac_key: String) -> BlueprintResult<Self> {
        if instance_id.is_empty()
            || instance_id.len() > 256
            || instance_id
                .chars()
                .any(|v| v.is_control() || v.is_whitespace())
            || hmac_key.is_empty()
            || hmac_key.len() > 4096
            || hmac_key.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        let http = ProviderHttpClient::new(
            gateway_url.trim_end_matches('/'),
            Duration::from_secs(15),
            1024 * 1024,
        )?;
        let request = http
            .request(Method::GET, &["instance-gateway", "_"])?
            .build()
            .map_err(|_| invalid())?;
        let mount_prefix = request
            .url()
            .path()
            .strip_suffix("/_")
            .ok_or_else(invalid)?
            .to_owned();
        Ok(Self {
            http,
            instance_id: Arc::new(instance_id),
            secret: Arc::new(Zeroizing::new(hmac_key)),
            mount_prefix: Arc::new(mount_prefix),
        })
    }
    /// Sends a JSON capability request, binding the router-relative path and
    /// the exact serialized body, as `createInstanceGatewayTransport` does.
    /// # Errors
    /// Returns sanitized transport/configuration errors or a bounded response.
    pub async fn request_json(
        &self,
        method: Method,
        route: &[&str],
        body: Option<&Value>,
    ) -> BlueprintResult<ProviderResponse> {
        let mut request = self.request(method, route)?;
        if let Some(body) = body {
            request = request.json(body);
        }
        self.execute(request, false, false).await
    }
    /// Sends a provider form request. Stripe gateway signatures bind the full
    /// router-relative query as well as exact form bytes; empty forms are absent.
    /// # Errors
    /// Returns sanitized transport/configuration errors or a bounded response.
    pub async fn request_form(
        &self,
        method: Method,
        route: &[&str],
        fields: &[(String, String)],
        version: Option<&str>,
    ) -> BlueprintResult<ProviderResponse> {
        let mut request = self.request(method.clone(), route)?;
        if method == Method::GET {
            request = request.query(fields);
        } else {
            request = request.form(fields);
        }
        if let Some(version) = version {
            request = request.header("stripe-version", version);
        }
        self.execute(request, true, false).await
    }
    /// Sends a bodyless capability request allowing an empty success response.
    /// # Errors
    /// Returns sanitized errors or a bounded response; nonempty bodies must be JSON.
    pub async fn request_json_or_empty(
        &self,
        method: Method,
        route: &[&str],
    ) -> BlueprintResult<ProviderResponse> {
        self.execute(self.request(method, route)?, false, true)
            .await
    }
    fn request(&self, method: Method, route: &[&str]) -> BlueprintResult<RequestBuilder> {
        if route.is_empty() || route.len() > 64 {
            return Err(invalid());
        }
        let mut segments = vec!["instance-gateway"];
        segments.extend_from_slice(route);
        self.http.request(method, &segments)
    }
    async fn execute(
        &self,
        request: RequestBuilder,
        bind_query: bool,
        allow_empty: bool,
    ) -> BlueprintResult<ProviderResponse> {
        let wire = request
            .try_clone()
            .ok_or_else(invalid)?
            .build()
            .map_err(|_| invalid())?;
        let mut path = wire
            .url()
            .path()
            .strip_prefix(self.mount_prefix.as_str())
            .ok_or_else(invalid)?
            .to_owned();
        if bind_query && let Some(query) = wire.url().query() {
            path.push('?');
            path.push_str(query);
        }
        let bytes = wire
            .body()
            .map(|body| body.as_bytes().ok_or_else(invalid))
            .transpose()?
            .unwrap_or_default();
        if bytes.len() > 1024 * 1024 || path.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let body = if bytes.is_empty() {
            SignedBody::Absent
        } else {
            SignedBody::Bytes(bytes)
        };
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let authorization = generate_hmac_auth_header(
            self.secret.as_bytes(),
            &self.instance_id,
            wire.method().as_str(),
            &path,
            body,
            &timestamp,
            &uuid::Uuid::new_v4().to_string(),
        )
        .map_err(|_| invalid())?;
        let request = request.header("authorization", authorization);
        if allow_empty {
            self.http.execute_json_or_empty(request).await
        } else {
            self.http.execute_json(request).await
        }
    }
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "instance_gateway_configuration",
        "Invalid instance gateway configuration or request",
    )
}

/// Sanitized platform refusal/transport/typed-response failure.
#[derive(Debug, Clone, PartialEq)]
pub struct InstanceGatewayRequestError {
    pub code: String,
    pub status: Option<u16>,
    pub retry_after_seconds: Option<f64>,
}
impl std::fmt::Display for InstanceGatewayRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Instance gateway request failed ({})", self.code)?;
        if let Some(status) = self.status {
            write!(f, " (status {status})")?;
        }
        Ok(())
    }
}
impl std::error::Error for InstanceGatewayRequestError {}
impl From<BlueprintError> for InstanceGatewayRequestError {
    fn from(error: BlueprintError) -> Self {
        Self {
            code: error.code,
            status: None,
            retry_after_seconds: None,
        }
    }
}
impl InstanceGatewayRequestError {
    #[must_use]
    pub fn validation(code: &str) -> Self {
        Self {
            code: code.into(),
            status: None,
            retry_after_seconds: None,
        }
    }
}
impl InstanceGatewayClient {
    /// # Errors
    /// Preserves failure status/retry hints and validates the success DTO without
    /// exposing remote error text. No automatic retry is performed.
    pub async fn json<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        route: &[&str],
        body: Option<&Value>,
    ) -> Result<T, InstanceGatewayRequestError> {
        let response = self.request_json(method, route, body).await?;
        if !response.is_success() {
            return Err(InstanceGatewayRequestError {
                code: "instance_gateway_request_failed".into(),
                status: Some(response.status),
                retry_after_seconds: response.retry_after_seconds,
            });
        }
        serde_json::from_value(response.body).map_err(|_| {
            InstanceGatewayRequestError::validation("instance_gateway_invalid_response")
        })
    }
}
