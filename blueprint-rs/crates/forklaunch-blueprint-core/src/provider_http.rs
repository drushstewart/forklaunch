//! Bounded provider HTTP transport. Credentials are attached by typed clients;
//! redirects/retries are disabled and remote origins require TLS.
use crate::{BlueprintError, BlueprintResult};
use reqwest::{Client, Method, RequestBuilder, redirect::Policy};
use serde_json::Value;
use std::time::Duration;
use url::Url;
#[derive(Clone)]
pub struct ProviderHttpClient {
    client: Client,
    base: Url,
    max_bytes: usize,
}
pub struct ProviderResponse {
    pub status: u16,
    pub body: Value,
    /// Positive finite Retry-After seconds; date-form headers are not interpreted.
    pub retry_after_seconds: Option<f64>,
}
impl ProviderResponse {
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }
}
impl std::fmt::Debug for ProviderHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderHttpClient")
            .field("base", &self.base)
            .field("max_response_bytes", &self.max_bytes)
            .finish_non_exhaustive()
    }
}
impl ProviderHttpClient {
    /// # Errors
    /// Rejects remote plaintext, credentials, queries, fragments and invalid bounds.
    pub fn new(base: &str, timeout: Duration, max_response_bytes: usize) -> BlueprintResult<Self> {
        Self::with_local_mock_policy(base, timeout, max_response_bytes, false)
    }
    /// Explicit development-only transport to the fixed Docker gateway mock.
    /// Production callers should use `new`, which requires remote TLS.
    /// # Errors
    /// Rejects invalid timeout/response bounds or client construction failures.
    pub fn local_gateway_mock(
        timeout: Duration,
        max_response_bytes: usize,
    ) -> BlueprintResult<Self> {
        Self::with_local_mock_policy(
            "http://gateway-mock:8080",
            timeout,
            max_response_bytes,
            true,
        )
    }
    fn with_local_mock_policy(
        base: &str,
        timeout: Duration,
        max_response_bytes: usize,
        local_mock: bool,
    ) -> BlueprintResult<Self> {
        let base = Url::parse(base).map_err(|_| config_error())?;
        let loopback = base.host_str().is_some_and(|h| {
            h == "localhost"
                || h.parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(base.scheme() == "https" || (base.scheme() == "http" && (loopback || local_mock)))
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || timeout.is_zero()
            || timeout > Duration::from_secs(120)
            || max_response_bytes == 0
            || max_response_bytes > 16 * 1024 * 1024
        {
            return Err(config_error());
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5).min(timeout))
            .timeout(timeout)
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .build()
            .map_err(|_| config_error())?;
        Ok(Self {
            client,
            base,
            max_bytes: max_response_bytes,
        })
    }
    /// Each segment is percent-encoded independently, preventing provider IDs
    /// from changing the trusted endpoint's path, query, origin or credentials.
    /// # Errors
    /// Rejects empty/dot segments and opaque endpoint URLs.
    pub fn request(&self, method: Method, segments: &[&str]) -> BlueprintResult<RequestBuilder> {
        if segments
            .iter()
            .any(|segment| segment.is_empty() || matches!(*segment, "." | ".."))
        {
            return Err(config_error());
        }
        let mut url = self.base.clone();
        {
            let mut path = url.path_segments_mut().map_err(|()| config_error())?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(self.client.request(method, url))
    }
    /// Encodes dynamic path segments as JavaScript `encodeURIComponent` does.
    /// # Errors
    /// Rejects empty/dot segments and opaque endpoint URLs.
    pub fn request_uri_components(
        &self,
        method: Method,
        segments: &[&str],
    ) -> BlueprintResult<RequestBuilder> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        if segments
            .iter()
            .any(|segment| segment.is_empty() || matches!(*segment, "." | ".."))
        {
            return Err(config_error());
        }
        let mut url = self.base.clone();
        let mut path = url.path().trim_end_matches('/').to_owned();
        for segment in segments {
            path.push('/');
            for byte in segment.bytes() {
                if byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
                    )
                {
                    path.push(char::from(byte));
                } else {
                    path.push('%');
                    path.push(char::from(HEX[usize::from(byte >> 4)]));
                    path.push(char::from(HEX[usize::from(byte & 15)]));
                }
            }
        }
        url.set_path(&path);
        Ok(self.client.request(method, url))
    }
    /// # Errors
    /// Rejects network failures, oversized bodies and malformed success JSON.
    /// Failure-status bodies are discarded to avoid exposing provider PII/secrets.
    pub async fn execute_json(&self, request: RequestBuilder) -> BlueprintResult<ProviderResponse> {
        self.execute(request, false).await
    }
    /// Like `execute_json`, but permits an empty successful body (voice hang-up).
    /// # Errors
    /// Rejects network failures, oversized bodies and nonempty malformed JSON.
    pub async fn execute_json_or_empty(
        &self,
        request: RequestBuilder,
    ) -> BlueprintResult<ProviderResponse> {
        self.execute(request, true).await
    }
    async fn execute(
        &self,
        request: RequestBuilder,
        allow_empty: bool,
    ) -> BlueprintResult<ProviderResponse> {
        let mut response = request.send().await.map_err(|_| transport_error())?;
        let status = response.status().as_u16();
        let retry_after_seconds = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0);
        if !response.status().is_success() {
            return Ok(ProviderResponse {
                status,
                body: Value::Null,
                retry_after_seconds,
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.max_bytes as u64)
        {
            return Err(transport_error());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| transport_error())? {
            if chunk.len() > self.max_bytes.saturating_sub(bytes.len()) {
                return Err(transport_error());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = if allow_empty && bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| transport_error())?
        };
        Ok(ProviderResponse {
            status,
            body,
            retry_after_seconds,
        })
    }
}
fn config_error() -> BlueprintError {
    BlueprintError::new(
        "provider_invalid_config",
        "Invalid provider HTTP configuration",
    )
}
fn transport_error() -> BlueprintError {
    BlueprintError::new("provider_transport_error", "Provider HTTP request failed")
}
/// Sanitized provider error. No response bodies, credentials or URLs are included.
#[must_use]
pub fn provider_status_error(provider: &str, operation: &str, status: u16) -> BlueprintError {
    BlueprintError::new(
        "provider_request_failed",
        format!("{provider} {operation} failed (status {status})"),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_require_tls_and_cannot_escape_with_provider_ids() {
        for base in [
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com?query",
            "https://example.com#fragment",
            "file:///tmp/provider",
        ] {
            assert!(ProviderHttpClient::new(base, Duration::from_secs(15), 1024).is_err());
        }
        assert!(
            ProviderHttpClient::new("http://gateway-mock:8080", Duration::from_secs(15), 1024)
                .is_err()
        );
        let local_mock =
            ProviderHttpClient::local_gateway_mock(Duration::from_secs(15), 1024).unwrap();
        assert_eq!(
            local_mock
                .request(Method::GET, &["instance-gateway", "models"])
                .unwrap()
                .build()
                .unwrap()
                .url()
                .as_str(),
            "http://gateway-mock:8080/instance-gateway/models"
        );
        let client = ProviderHttpClient::new(
            "http://127.0.0.1:1234/prefix/",
            Duration::from_secs(15),
            1024,
        )
        .unwrap();
        let request = client
            .request(Method::GET, &["orders", "id/with?query#fragment"])
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(
            request.url().as_str(),
            "http://127.0.0.1:1234/prefix/orders/id%2Fwith%3Fquery%23fragment"
        );
        assert!(client.request(Method::GET, &[".."]).is_err());
        assert!(ProviderHttpClient::new("https://example.com", Duration::ZERO, 1024).is_err());
        assert!(
            ProviderHttpClient::new("https://example.com", Duration::from_secs(15), 0).is_err()
        );
    }
}
