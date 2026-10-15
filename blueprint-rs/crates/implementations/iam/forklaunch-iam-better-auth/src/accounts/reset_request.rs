//! Opt-in trusted reset delivery; tokens commit before callback preparation.
//! Configured `PostgreSQL` encrypts records; custom stores own storage security.
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model, SecondsFormat,
    Utc, Value, json, public_user, validate_email,
};
use crate::{
    background::{AuthBackgroundDispatcher, DeliveryFuture},
    origins::OriginPolicy,
};
use rand_core::{OsRng, RngCore};
use std::sync::Arc;
use zeroize::Zeroizing;
/// Sensitive owned delivery input. No Debug/Serde or session/credential metadata.
/// User fields use the existing public projection, with scalar/size validation.
pub struct PasswordResetDeliveryContext {
    user: Value,
    url: Zeroizing<String>,
    token: Zeroizing<String>,
}
impl PasswordResetDeliveryContext {
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }
}
/// Trusted delivery construction. Returned futures are owned, lazy and fallible.
pub trait PasswordResetDelivery: Send + Sync {
    /// # Errors
    /// Preparation failure returns an empty HTTP500 after token persistence. Async
    /// errors are sanitized/handled; panics and blocking code are outside this contract.
    fn prepare(&self, context: PasswordResetDeliveryContext) -> BlueprintResult<DeliveryFuture>;
}
/// Tenant-bound reset request service. No delivery is enabled by default on HTTP.
pub struct PasswordResetService<S> {
    time_source: crate::time_source::TimeSource,
    token_lifetimes: super::AuthTokenLifetimes,
    store: Arc<S>,
    base_url: String,
    policy: Arc<OriginPolicy>,
    delivery: Arc<dyn PasswordResetDelivery>,
    dispatcher: Option<Arc<dyn AuthBackgroundDispatcher>>,
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "better_auth_invalid_config",
        "Invalid password reset delivery configuration",
    )
}
fn storage() -> BlueprintError {
    BlueprintError::new(
        "better_auth_store_error",
        "Unable to request password reset",
    )
}
fn token() -> BlueprintResult<Zeroizing<String>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut output = Zeroizing::new(String::with_capacity(24));
    while output.len() < 24 {
        let mut bytes = [0u8; 32];
        OsRng.try_fill_bytes(&mut bytes).map_err(|_| storage())?;
        for byte in bytes {
            if byte < 248 {
                output.push(char::from(ALPHABET[usize::from(byte % 62)]));
                if output.len() == 24 {
                    break;
                }
            }
        }
    }
    Ok(output)
}
pub(crate) fn projection(user: &Value) -> BlueprintResult<Value> {
    // Match public_user's allowlist, validating borrowed values before that helper
    // clones them. Do not recursively clone malformed caller-controlled fields.
    for field in [
        "id",
        "name",
        "email",
        "emailVerified",
        "image",
        "createdAt",
        "updatedAt",
        "firstName",
        "lastName",
        "phoneNumber",
        "twoFactorEnabled",
    ] {
        let Some(value) = user.get(field) else {
            continue;
        };
        let valid = if matches!(field, "emailVerified" | "twoFactorEnabled") {
            value.is_boolean() || value.is_null()
        } else {
            value.is_null() || value.as_str().is_some_and(|text| text.len() <= 4096)
        };
        if !valid {
            return Err(storage());
        }
    }
    Ok(public_user(user))
}

pub(crate) fn component(value: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            output.push(char::from(byte));
        } else {
            output.push('%');
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    output
}
fn success() -> Value {
    json!({"status":true,"message":"If this email exists in our system, check your email for the reset link"})
}
impl<S: AuthStore> PasswordResetService<S> {
    #[cfg(feature = "http")]
    pub(crate) fn with_time_source(mut self, source: crate::time_source::TimeSource) -> Self {
        self.time_source = source;
        self
    }

    #[must_use]
    pub fn with_token_lifetimes(mut self, lifetimes: super::AuthTokenLifetimes) -> Self {
        self.token_lifetimes = lifetimes;
        self
    }
    /// # Errors
    /// Rejects non-HTTP(S), credential-bearing/query/fragment URLs or a base above8192 bytes.
    pub fn new(
        store: Arc<S>,
        mut base_url: String,
        policy: Arc<OriginPolicy>,
        delivery: Arc<dyn PasswordResetDelivery>,
    ) -> BlueprintResult<Self> {
        let parsed = url::Url::parse(&base_url).map_err(|_| invalid())?;
        if base_url.len() > 8192
            || !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid());
        }
        base_url.truncate(base_url.trim_end_matches('/').len());
        Ok(Self {
            time_source: crate::time_source::TimeSource::default(),
            token_lifetimes: super::AuthTokenLifetimes::default(),
            store,
            base_url,
            policy,
            delivery,
            dispatcher: None,
        })
    }
    #[must_use]
    pub fn with_background_dispatcher(
        mut self,
        dispatcher: Arc<dyn AuthBackgroundDispatcher>,
    ) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }
    /// Requests a reset link (one hour by default). Missing users get the same response without
    /// delivery; dummy work is timing mitigation, not a constant-time guarantee.
    /// # Errors
    /// Rejects invalid email/untrusted redirects before storage, persistence errors,
    /// malformed public user data, or synchronous delivery preparation failure.
    pub async fn request(
        &self,
        email: &str,
        redirect: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        validate_email(email)?;
        if redirect.len() > 8192 || (!redirect.is_empty() && !self.policy.trusted(redirect, true)) {
            return Err(BlueprintError::new(
                "INVALID_REDIRECT_URL",
                "Invalid redirect URL",
            ));
        }
        let _ = now
            .checked_add_signed(chrono::Duration::seconds(i64::from(
                self.token_lifetimes.password_reset_seconds(),
            )))
            .ok_or_else(invalid)?;
        let mut tx = self.store.begin(true).await?;
        let user = tx
            .find_unique(Model::User, "email", &email.to_lowercase())
            .await?;
        let proof = token()?;
        let Some(user) = user else {
            let _ = tx
                .find_page(
                    Model::Verification,
                    "identifier",
                    "dummy-verification-token",
                    None,
                    1,
                )
                .await?;
            tx.commit().await?;
            return Ok(success());
        };
        let public = projection(&user)?;
        let now = self.time_source.sample(now);
        let expires = now
            .checked_add_signed(chrono::Duration::seconds(i64::from(
                self.token_lifetimes.password_reset_seconds(),
            )))
            .ok_or_else(invalid)?;
        let identifier = format!("reset-password:{}", proof.as_str());
        let stamp = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        tx.insert(Model::Verification,json!({"id":uuid::Uuid::new_v4().to_string(),"identifier":identifier,"value":super::text(&user,"id")?,"expiresAt":expires.to_rfc3339_opts(SecondsFormat::Millis,true),"createdAt":stamp,"updatedAt":stamp})).await?;
        tx.commit().await?;
        let url = Zeroizing::new(format!(
            "{}/reset-password/{}?callbackURL={}",
            self.base_url,
            proof.as_str(),
            component(redirect)
        ));
        let future = self
            .delivery
            .prepare(PasswordResetDeliveryContext {
                user: public,
                url,
                token: proof,
            })
            .map_err(|_| {
                BlueprintError::new(
                    "PASSWORD_RESET_DELIVERY_PREPARATION_FAILED",
                    "Unable to prepare password reset delivery",
                )
            })?;
        crate::background::run(
            future,
            self.dispatcher.as_deref(),
            "/request-password-reset",
            "PASSWORD_RESET_DELIVERY_FAILED",
            "PASSWORD_RESET_DISPATCH_FAILED",
            "Password reset delivery failed",
        )
        .await;
        Ok(success())
    }
}

#[cfg(test)]
pub(crate) mod tests;
