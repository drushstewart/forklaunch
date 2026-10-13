//! Signed browser correlation, bounded state expiry and atomic one-time consumption.
use crate::{
    cookies::{SessionCookie, cookie_value, sign_cookie_value, verify_cookie_value},
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use rand_core::RngCore;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

/// Trusted state storage. Redirect destinations must be origin-validated by the caller.
pub struct StateService<S> {
    store: Arc<S>,
    secret: Zeroizing<Vec<u8>>,
    cookie: SessionCookie,
}
/// Server-owned PKCE verifier. Only its challenge belongs on the provider URL.
pub struct StateGrant {
    pub state: String,
    pub code_verifier: Zeroizing<String>,
    pub set_cookie: String,
}
/// Validated state and the separate response cookie header clearing browser correlation.
pub struct ConsumedState {
    pub data: Value,
    pub clear_cookie: String,
}
impl<S: AuthStore> StateService<S> {
    /// # Errors
    /// Rejects signing secrets shorter than the configured minimum.
    pub fn new(store: Arc<S>, secret: Vec<u8>, secure: bool) -> BlueprintResult<Self> {
        if secret.len() < 32 {
            return Err(error(
                "better_auth_invalid_config",
                "Authentication secret must contain at least 32 bytes",
            ));
        }
        Ok(Self {
            store,
            secret: Zeroizing::new(secret),
            cookie: SessionCookie { secure },
        })
    }
    /// Persists trusted state for ten minutes and signs a five-minute browser cookie.
    /// # Errors
    /// Rejects invalid state fields, time overflow, randomness or storage failures.
    pub async fn generate(
        &self,
        mut data: Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<StateGrant> {
        let verifier = Zeroizing::new(random(128)?);
        let state = random(32)?;
        let expires = now
            .checked_add_signed(Duration::minutes(10))
            .ok_or_else(internal)?;
        if !data.is_object() {
            return Err(internal());
        }
        data["codeVerifier"] = json!(*verifier);
        data["expiresAt"] = json!(expires.timestamp_millis());
        data["oauthState"] = json!(state);
        validate(&mut data)?;
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut tx = self.store.begin(true).await?;
        tx.insert(Model::Verification, json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":state,
            "value":crate::ecmascript::stringify(&data),"expiresAt":expires.to_rfc3339_opts(SecondsFormat::Millis, true),
            "createdAt":timestamp,"updatedAt":timestamp})).await?;
        tx.commit().await?;
        Ok(StateGrant {
            set_cookie: self.cookie.named_header(
                self.name(),
                &sign_cookie_value(&state, &self.secret),
                Some(300),
            ),
            state,
            code_verifier: verifier,
        })
    }
    /// Validates correlation before consuming state. Failed correlation retains live state.
    /// Expired verification cleanup persists even when later validation fails.
    /// # Errors
    /// Returns state-not-found/mismatch/invalid or sanitized storage failures. Expiry
    /// errors carry `clearCookie: true` because the valid browser cookie was consumed.
    pub async fn consume(
        &self,
        state: &str,
        cookie_header: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<ConsumedState> {
        if state.is_empty() {
            return Err(error(
                "state_not_found",
                "State not found in OAuth callback",
            ));
        }
        let mut tx = self.store.begin(true).await?;
        let rows =
            crate::store::find_all(&mut tx, Model::Verification, "identifier", state).await?;
        let mut selected: Option<(DateTime<Utc>, Value)> = None;
        for row in &rows {
            if row["identifier"] == state {
                let created = time(row, "createdAt")?;
                if selected.as_ref().is_none_or(|(best, _)| created > *best) {
                    selected = Some((created, row.clone()));
                }
            }
        }
        let mut after = None;
        loop {
            let page = tx.list_page(Model::Verification, after, 256).await?;
            for row in page.records {
                if time(&row, "expiresAt")? < now {
                    tx.delete(Model::Verification, text(&row, "id")?).await?;
                }
            }
            after = page.next;
            if after.is_none() {
                break;
            }
        }
        let validation = selected.as_ref().map_or_else(
            || {
                Err(error(
                    "state_mismatch",
                    "State mismatch: verification not found",
                ))
            },
            |(_, row)| {
                let mut data =
                    crate::ecmascript::parse(text(row, "value")?).map_err(|_| internal())?;
                validate(&mut data)?;
                if data.get("oauthState").is_some_and(|stored| stored != state) {
                    return Err(with_destination(
                        error(
                            "state_security_mismatch",
                            "State mismatch: OAuth state parameter does not match stored state",
                        ),
                        &data,
                    ));
                }
                let valid = cookie_header
                    .and_then(|header| cookie_value(header, self.name()))
                    .is_some_and(|cookie| {
                        verify_cookie_value(&cookie, &self.secret) == Some(state)
                    });
                if !valid {
                    return Err(with_destination(
                        error(
                            "state_security_mismatch",
                            "State mismatch: State not persisted correctly",
                        ),
                        &data,
                    ));
                }
                Ok(data)
            },
        );
        let data = match validation {
            Ok(data) => data,
            Err(error) => {
                tx.commit().await?;
                return Err(error);
            }
        };
        for row in rows.iter().filter(|row| row["identifier"] == state) {
            tx.delete(Model::Verification, text(row, "id")?).await?;
        }
        tx.commit().await?;
        let now_millis = json!(now.timestamp_millis())
            .as_f64()
            .ok_or_else(internal)?;
        if data["expiresAt"].as_f64().ok_or_else(internal)? < now_millis {
            return Err(with_destination(
                error("state_mismatch", "Invalid state: request expired"),
                &data,
            )
            .with_detail("clearCookie", json!(true)));
        }
        Ok(ConsumedState {
            data,
            clear_cookie: self.cookie.named_header(self.name(), "", Some(0)),
        })
    }
    #[cfg(feature = "http")]
    pub(crate) fn clear_cookie(&self) -> String {
        self.cookie.named_header(self.name(), "", Some(0))
    }
    fn name(&self) -> &'static str {
        if self.cookie.secure {
            "__Secure-better-auth.state"
        } else {
            "better-auth.state"
        }
    }
}
fn validate(data: &mut Value) -> BlueprintResult<()> {
    if !data.is_object()
        || !data["callbackURL"].is_string()
        || !data["codeVerifier"].is_string()
        || !data["expiresAt"].as_f64().is_some_and(f64::is_finite)
        || ["errorURL", "newUserURL", "oauthState", "idTokenNonce"]
            .iter()
            .any(|field| data.get(*field).is_some_and(|value| !value.is_string()))
        || data
            .get("requestSignUp")
            .is_some_and(|value| !value.is_boolean())
        || data
            .get("serverContext")
            .is_some_and(|value| !value.is_object())
        || data
            .get("link")
            .is_some_and(|value| !value.is_object() || !value["email"].is_string())
    {
        return Err(internal());
    }
    if let Some(link) = data.get_mut("link") {
        let user_id = crate::ecmascript::string(link.get("userId")).ok_or_else(internal)?;
        *link = json!({"email":link["email"],"userId":user_id});
    }
    Ok(())
}
fn time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text(row, field)?)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|_| internal())
}
fn text<'a>(row: &'a Value, field: &str) -> BlueprintResult<&'a str> {
    row[field].as_str().ok_or_else(internal)
}
fn with_destination(error: BlueprintError, data: &Value) -> BlueprintError {
    if let Some(url) = data.get("errorURL") {
        error.with_detail("errorURL", url.clone())
    } else {
        error
    }
}

fn internal() -> BlueprintError {
    error("internal_server_error", "Invalid OAuth state")
}
fn error(code: &str, message: &str) -> BlueprintError {
    BlueprintError::new(code, message)
}
fn random(length: usize) -> BlueprintResult<String> {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut result = String::with_capacity(length);
    while result.len() < length {
        let mut bytes = [0; 64];
        rand_core::OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| internal())?;
        for byte in bytes {
            if byte < 248 {
                result.push(char::from(ALPHABET[usize::from(byte) % 62]));
            }
            if result.len() == length {
                break;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
pub(crate) mod tests;
