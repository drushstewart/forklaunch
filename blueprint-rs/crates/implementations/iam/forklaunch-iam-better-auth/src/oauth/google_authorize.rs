//! Configured Google authorization URL and S256 PKCE wire protocol.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Trusted provider credentials and the configured absolute callback URI.
/// Private client credentials are never formatted or serialized.
pub struct GoogleAuthorization {
    client_id: String,
    client_secret: Zeroizing<String>,
    redirect_uri: String,
}
impl GoogleAuthorization {
    #[must_use]
    pub fn new(client_id: String, client_secret: String, redirect_uri: String) -> Self {
        Self {
            client_id,
            client_secret: Zeroizing::new(client_secret),
            redirect_uri,
        }
    }
    /// Builds the configured Google authorization request. HTTP callers validate
    /// additional parameters before persisting state; reserved keys are also ignored here.
    /// # Errors
    /// Rejects missing configured credentials or PKCE verifier.
    pub fn authorization_url(
        &self,
        state: &str,
        code_verifier: &str,
        scopes: &[String],
        login_hint: Option<&str>,
        additional_params: &[(String, String)],
    ) -> BlueprintResult<String> {
        if self.client_id.is_empty() || self.client_secret.is_empty() {
            return Err(error("CLIENT_ID_AND_SECRET_REQUIRED"));
        }
        if code_verifier.is_empty() {
            return Err(error("codeVerifier is required for Google"));
        }
        let scopes = [
            vec![
                "email".to_string(),
                "profile".to_string(),
                "openid".to_string(),
            ],
            scopes.to_vec(),
        ]
        .concat()
        .join(" ");
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
        let mut pairs = vec![
            ("response_type".to_string(), "code".to_string()),
            ("client_id".to_string(), self.client_id.clone()),
            ("state".to_string(), state.to_string()),
            ("scope".to_string(), scopes),
            ("redirect_uri".to_string(), self.redirect_uri.clone()),
        ];
        if let Some(hint) = login_hint.filter(|hint| !hint.is_empty()) {
            pairs.push(("login_hint".to_string(), hint.to_string()));
        }
        pairs.push(("code_challenge_method".to_string(), "S256".to_string()));
        pairs.push(("code_challenge".to_string(), challenge));
        // Google spreads defaults into a JavaScript object before Object.entries:
        // canonical array-index keys enumerate before the ordinary default key.
        let mut extras = vec![("include_granted_scopes".to_string(), "true".to_string())];
        for (key, value) in additional_params {
            set(&mut extras, key, value);
        }
        extras.sort_by_key(|(key, _)| {
            key.parse::<u32>()
                .ok()
                .filter(|index| *index != u32::MAX && index.to_string() == *key)
                .map_or(u64::MAX, u64::from)
        });
        for (key, value) in extras {
            if !RESERVED.contains(&key.as_str()) {
                set(&mut pairs, &key, &value);
            }
        }
        let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|_| error("Invalid Google authorization endpoint"))?;
        url.query_pairs_mut().extend_pairs(pairs);
        Ok(url.into())
    }
}
fn set(pairs: &mut Vec<(String, String)>, key: &str, value: &str) {
    if let Some((_, previous)) = pairs.iter_mut().find(|(name, _)| name == key) {
        value.clone_into(previous);
    } else {
        pairs.push((key.to_string(), value.to_string()));
    }
}
pub(crate) const RESERVED: &[&str] = &[
    "state",
    "client_id",
    "redirect_uri",
    "response_type",
    "code_challenge",
    "code_challenge_method",
    "nonce",
    "scope",
];
fn error(message: &str) -> BlueprintError {
    BlueprintError::new("better_auth_google_authorization", message)
}
