#[cfg(any(feature = "http", test))]
use super::Arc;
use super::{
    AuthGrant, AuthStore, BlueprintError, BlueprintResult, CredentialService, DateTime,
    SignupInput, Utc, Value, json, public_user,
};
#[cfg(feature = "http")]
use super::{EmailVerificationDelivery, Zeroizing};
/// Trusted credential policy, separate from installing manual email delivery.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EmailVerificationPolicy {
    pub require: bool,
    /// None follows `require`; explicit false suppresses signup delivery.
    pub send_on_signup: Option<bool>,
    /// Applies only to a correct-password, unverified, verification-required login.
    pub send_on_signin: bool,
}
/// Signup can succeed without granting an authenticated session.
pub enum SignupOutcome {
    Authenticated(AuthGrant),
    VerificationRequired {
        user: Value,
    },
    /// Signup auto-signin was disabled independently of verification.
    Unauthenticated {
        user: Value,
    },
}
impl SignupOutcome {
    #[must_use]
    pub fn body(&self) -> Value {
        match self {
            Self::Authenticated(grant) => grant.signup_body(),
            Self::VerificationRequired { user } | Self::Unauthenticated { user } => {
                json!({"token":null,"user":user})
            }
        }
    }
}
pub(super) fn required() -> BlueprintError {
    BlueprintError::new("EMAIL_NOT_VERIFIED", "Email not verified")
}
pub(super) fn signup_disabled() -> BlueprintError {
    BlueprintError::new(
        "SIGNUP_SESSION_DISABLED",
        "Use the signup outcome API when signup auto-signin is disabled",
    )
}
pub(super) fn without_session(user: Value, required: bool) -> SignupOutcome {
    if required {
        SignupOutcome::VerificationRequired { user }
    } else {
        SignupOutcome::Unauthenticated { user }
    }
}
pub(super) fn synthetic(user: &Value, required: bool) -> SignupOutcome {
    let mut user = public_user(user);
    for field in ["image", "phoneNumber"] {
        if user.get(field).is_none() {
            user[field] = Value::Null;
        }
    }
    without_session(user, required)
}
#[cfg(feature = "http")]
pub(crate) struct Delivery {
    pub hook: Arc<dyn EmailVerificationDelivery>,
    pub dispatcher: Option<Arc<dyn crate::background::AuthBackgroundDispatcher>>,
    pub base: String,
    pub secret: Zeroizing<Vec<u8>>,
}
impl<S: AuthStore> CredentialService<S> {
    /// Controls session issuance only for signup. Subsequent signin is unaffected.
    #[must_use]
    pub fn with_signup_auto_sign_in(mut self, enabled: bool) -> Self {
        self.signup_auto_sign_in = enabled;
        self
    }
    pub(super) fn skip_signup_session(&self) -> bool {
        !self.signup_auto_sign_in || self.verification_policy.require
    }
    #[must_use]
    pub fn with_email_verification_policy(mut self, policy: EmailVerificationPolicy) -> Self {
        self.verification_policy = policy;
        self
    }
    /// Creates an atomic account with an explicit no-session result when verification
    /// is required or signup auto-signin is disabled. Legacy `sign_up` rejects
    /// either no-session configuration before any effects.
    /// # Errors
    /// Returns credential, callback preparation or persistence failures.
    pub async fn sign_up_outcome(
        &self,
        input: SignupInput,
        ip: &str,
        agent: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<SignupOutcome> {
        self.signup_outcome_with_fields(input, serde_json::Map::new(), ip, agent, now, "")
            .await
    }
    #[cfg(feature = "http")]
    pub(super) async fn deliver_verification(
        &self,
        user: &Value,
        callback: &str,
        now: DateTime<Utc>,
        route: &'static str,
    ) -> BlueprintResult<()> {
        if let Some(delivery) = &self.verification_delivery {
            let future = super::email_delivery::prepare(
                delivery.hook.as_ref(),
                user,
                &delivery.base,
                callback,
                &delivery.secret,
                now,
                self.token_lifetimes.email_verification_seconds(),
            )?;
            crate::background::run(
                future,
                delivery.dispatcher.as_deref(),
                route,
                "EMAIL_VERIFICATION_DELIVERY_FAILED",
                "EMAIL_VERIFICATION_DISPATCH_FAILED",
                "Email verification delivery failed",
            )
            .await;
        }
        Ok(())
    }
    #[cfg(not(feature = "http"))]
    #[allow(clippy::unused_self)] // Same callsite API; HTTP owns optional delivery configuration.
    pub(super) fn deliver_verification(
        &self,
        _user: &Value,
        _callback: &str,
        _now: DateTime<Utc>,
        _route: &'static str,
    ) -> std::future::Ready<BlueprintResult<()>> {
        std::future::ready(Ok(()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::accounts::SigninInput;
    use crate::store::{AuthTransaction, Model, memory::MemoryAuthStore};
    fn input() -> SignupInput {
        serde_json::from_value(json!({"email":"policy@example.test","name":"Submitted","firstName":"First","lastName":"Last","password":"owned-policy-password"})).unwrap()
    }
    #[tokio::test]
    async fn verification_policy_required_no_hook_legacy_failclosed_and_duplicate_privacy() {
        lifecycle(Arc::new(MemoryAuthStore::default())).await;
    }
    pub(crate) async fn lifecycle<S: AuthStore>(store: Arc<S>) {
        let service = CredentialService::new(store.clone()).with_email_verification_policy(
            EmailVerificationPolicy {
                require: true,
                ..Default::default()
            },
        );
        let now = Utc::now();
        assert_eq!(
            service
                .sign_up(input(), "", "", now)
                .await
                .err()
                .unwrap()
                .code,
            "EMAIL_NOT_VERIFIED"
        );
        let first = service
            .sign_up_outcome(input(), "", "", now)
            .await
            .unwrap()
            .body();
        assert!(first["token"].is_null());
        let duplicate = service
            .sign_up_outcome(input(), "", "", now)
            .await
            .unwrap()
            .body();
        assert!(duplicate["token"].is_null());
        assert_ne!(duplicate["user"]["id"], first["user"]["id"]);
        let mut tx = store.begin(true).await.unwrap();
        let mut user = tx
            .get(Model::User, first["user"]["id"].as_str().unwrap())
            .await
            .unwrap()
            .unwrap();
        user["twoFactorEnabled"] = json!(true);
        tx.replace(Model::User, user).await.unwrap();
        tx.commit().await.unwrap();
        let signin = SigninInput {
            email: "policy@example.test".into(),
            password: "owned-policy-password".into(),
            callback_url: None,
            remember_me: true,
        };
        assert_eq!(
            service
                .sign_in(signin, "", "", now)
                .await
                .err()
                .unwrap()
                .code,
            "EMAIL_NOT_VERIFIED"
        );
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.find_page(
                Model::Session,
                "userId",
                first["user"]["id"].as_str().unwrap(),
                None,
                10
            )
            .await
            .unwrap()
            .records
            .is_empty()
        );
        tx.commit().await.unwrap();
    }
}

#[cfg(all(test, feature = "http"))]
pub(crate) mod tests_async;

#[cfg(test)]
pub(crate) mod tests_auto;
