//! Bounded transport to trusted Google endpoints; request tokens cannot select URLs.
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::time::Duration;
use zeroize::Zeroizing;

pub struct GoogleClient {
    pub(crate) client_id: String,
    client_secret: Zeroizing<String>,
    certificates: url::Url,
    token_endpoint: url::Url,
    client: reqwest::Client,
}
impl GoogleClient {
    /// # Errors
    /// Rejects empty credentials or TLS client initialization failures.
    pub fn new(client_id: String, client_secret: String) -> BlueprintResult<Self> {
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(config());
        }
        Ok(Self {
            client_id,
            client_secret: Zeroizing::new(client_secret),
            certificates: url::Url::parse("https://www.googleapis.com/oauth2/v3/certs")
                .map_err(|_| config())?,
            token_endpoint: url::Url::parse("https://oauth2.googleapis.com/token")
                .map_err(|_| config())?,
            client: reqwest::Client::builder()
                .retry(reqwest::retry::never())
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .map_err(|_| config())?,
        })
    }
    /// Trusted operator certificate endpoint override, for proxies or local fixtures.
    /// # Errors
    /// Requires HTTPS or loopback HTTP, without credentials or URL fragments.
    pub fn with_certificate_endpoint(mut self, endpoint: &str) -> BlueprintResult<Self> {
        let url = endpoint_url(endpoint)?;
        self.certificates = url;
        Ok(self)
    }
    /// Trusted operator token endpoint override. Requests never select this URL.
    /// # Errors
    /// Requires HTTPS or loopback HTTP without URL credentials or fragments.
    pub fn with_token_endpoint(mut self, endpoint: &str) -> BlueprintResult<Self> {
        self.token_endpoint = endpoint_url(endpoint)?;
        Ok(self)
    }
    /// Exchanges a code using the server-owned PKCE verifier and credentials.
    /// # Errors
    /// Returns a sanitized failure on HTTP/protocol errors; never includes tokens.
    pub async fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        let body = Zeroizing::new({
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "authorization_code")
                .append_pair("code", code);
            if !verifier.is_empty() {
                form.append_pair("code_verifier", verifier);
            }
            form.append_pair("redirect_uri", redirect_uri)
                .append_pair("client_id", &self.client_id)
                .append_pair("client_secret", &self.client_secret);
            form.finish()
        });
        let response = self
            .client
            .post(self.token_endpoint.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| exchange_error())?;
        let data = bounded_json(response).await.ok_or_else(exchange_error)?;
        super::google_tokens::normalize(&data, now).ok_or_else(exchange_error)
    }
    pub(crate) fn authorization(
        &self,
        redirect_uri: String,
    ) -> super::google_authorize::GoogleAuthorization {
        super::google_authorize::GoogleAuthorization::new(
            self.client_id.clone(),
            self.client_secret.to_string(),
            redirect_uri,
        )
    }
    /// Verifies a client-submitted token using only the configured certificate endpoint.
    /// Network/protocol failures deliberately return the same invalid-token result.
    pub async fn verify(
        &self,
        token: &str,
        nonce: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<Value> {
        // Reject malformed/unsupported tokens before any outbound request.
        let (header, _, _, _) = crate::keyring::verify::compact(token)?;
        if header["alg"] != "RS256" {
            return None;
        }
        let response = self
            .client
            .get(self.certificates.clone())
            .send()
            .await
            .ok()?;
        let jwks = bounded_json(response).await?;
        super::google_verify::verify_google_sign_in_token(token, &self.client_id, nonce, &jwks, now)
    }
}
fn config() -> BlueprintError {
    BlueprintError::new(
        "better_auth_invalid_config",
        "Invalid Google provider configuration",
    )
}

async fn bounded_json(mut response: reqwest::Response) -> Option<Value> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > 1024 * 1024)
    {
        return None;
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len().checked_add(chunk.len())? > 1024 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = String::from_utf8_lossy(&bytes);
    Some(crate::ecmascript::parse(&text).unwrap_or_else(|_| Value::String(text.into_owned())))
}
fn endpoint_url(endpoint: &str) -> BlueprintResult<url::Url> {
    let url = url::Url::parse(endpoint).map_err(|_| config())?;
    let loopback = match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(config());
    }
    Ok(url)
}
fn exchange_error() -> BlueprintError {
    BlueprintError::new(
        "google_code_exchange_failed",
        "Unable to exchange Google authorization code",
    )
}
