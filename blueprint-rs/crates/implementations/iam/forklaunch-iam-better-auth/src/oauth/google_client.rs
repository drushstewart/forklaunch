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
            client: reqwest::Client::builder()
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
        let url = url::Url::parse(endpoint).map_err(|_| config())?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(config());
        }
        self.certificates = url;
        Ok(self)
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
        let mut response = self
            .client
            .get(self.certificates.clone())
            .send()
            .await
            .ok()?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|size| size > 1024 * 1024)
        {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len().checked_add(chunk.len())? > 1024 * 1024 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks = crate::ecmascript::parse(std::str::from_utf8(&bytes).ok()?).ok()?;
        super::google_verify::verify_google_sign_in_token(token, &self.client_id, nonce, &jwks, now)
    }
}
fn config() -> BlueprintError {
    BlueprintError::new(
        "better_auth_invalid_config",
        "Invalid Google provider configuration",
    )
}
