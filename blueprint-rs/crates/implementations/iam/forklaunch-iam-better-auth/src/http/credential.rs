use super::{AuthHttp, internal, set_cookie};
use crate::{
    accounts::{ChangePasswordInput, CredentialService, SigninInput, SignupInput, validate_length},
    store::AuthStore,
};
use axum::http::{HeaderMap, HeaderValue, header};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

#[allow(clippy::too_many_lines)] // One credential dispatch preserves cookie/MFA ordering.
pub(super) async fn login<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    mut payload: Value,
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let mut service = CredentialService::new(app.store.clone())
        .with_email_verification_policy(app.email_verification_policy)
        .with_signup_auto_sign_in(app.signup_auto_sign_in);
    if let Some(hook) = &app.email_verification_delivery {
        service.verification_delivery = Some(crate::accounts::verification_policy::Delivery {
            hook: hook.clone(),
            dispatcher: app.background_dispatcher.clone(),
            base: app.auth_base_url(),
            secret: app.secret.clone(),
        });
    }
    let (ip, agent) = super::request::client_metadata(headers);
    let grant = if path == "/sign-up/email" {
        validate_length(payload["password"].as_str().ok_or_else(internal)?, true)?;
        let mut profile = serde_json::Map::new();
        let object = payload.as_object_mut().ok_or_else(internal)?;
        for field in ["firstName", "lastName", "phoneNumber"] {
            if let Some(value) = object.remove(field) {
                profile.insert(field.into(), value);
            } else if field != "phoneNumber" {
                return Err(BlueprintError::new(
                    "MISSING_FIELD",
                    format!("{field} is required"),
                ));
            }
        }
        object.insert("firstName".into(), json!(""));
        object.insert("lastName".into(), json!(""));
        let callback = payload["callbackURL"].as_str().unwrap_or("").to_owned();
        let input: SignupInput = serde_json::from_value(payload).map_err(|_| internal())?;
        match service
            .signup_outcome_with_fields(input, profile, &ip, &agent, now, &callback)
            .await?
        {
            crate::accounts::SignupOutcome::Authenticated(grant) => grant,
            outcome @ (crate::accounts::SignupOutcome::VerificationRequired { .. }
            | crate::accounts::SignupOutcome::Unauthenticated { .. }) => {
                let mut body = outcome.body();
                if app.two_factor.is_some() {
                    super::two_factor::user_default(&mut body);
                }
                return Ok(body);
            }
        }
    } else {
        let input: SigninInput = serde_json::from_value(payload).map_err(|_| internal())?;
        if let Some(factor) = &app.two_factor {
            let trust = super::two_factor::signed(app, headers, "trust_device");
            match service
                .sign_in_with_two_factor(input, factor, trust.as_deref(), &ip, &agent, now)
                .await?
            {
                crate::two_factor::CredentialSigninOutcome::Authenticated { grant, trust } => {
                    if let Some(trust) = trust {
                        super::two_factor::cookie(
                            app,
                            pending,
                            "trust_device",
                            Some(&trust),
                            factor.lifetime_options().trusted_device().as_secs(),
                        )?;
                    }
                    grant
                }
                crate::two_factor::CredentialSigninOutcome::Challenge(challenge) => {
                    for cookie in app.cookie.clear_session().into_iter().take(2) {
                        set_cookie(pending, &cookie)?;
                    }
                    if !challenge.persistent {
                        set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
                    }
                    if trust.is_some() {
                        super::two_factor::cookie(app, pending, "trust_device", None, 0)?;
                    }
                    super::two_factor::cookie(
                        app,
                        pending,
                        "two_factor",
                        Some(&challenge.identifier),
                        factor.lifetime_options().challenge().as_secs(),
                    )?;
                    return Ok(
                        json!({"twoFactorRedirect":true,"twoFactorMethods":challenge.methods()}),
                    );
                }
            }
        } else {
            service.sign_in(input, &ip, &agent, now).await?
        }
    };
    set_cookie(
        pending,
        &app.cookie.serialize(
            grant.session["token"].as_str().ok_or_else(internal)?,
            &app.secret,
            grant.persistent,
        ),
    )?;
    if !grant.persistent {
        set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
    }
    if let Some(url) = &grant.callback_url
        && !url.is_empty()
    {
        pending.insert(
            header::LOCATION,
            HeaderValue::from_str(url).map_err(|_| internal())?,
        );
    }
    let mut body = if path == "/sign-up/email" {
        grant.signup_body()
    } else {
        grant.signin_body()
    };
    if app.two_factor.is_some() {
        super::two_factor::user_default(&mut body);
    }
    Ok(body)
}

pub(super) async fn authenticated<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    payload: Value,
    session: (&str, bool),
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let service = CredentialService::new(app.store.clone());
    let (token, transient) = session;
    match path {
        "/change-email" => {
            return Err(BlueprintError::new(
                "CHANGE_EMAIL_DISABLED",
                "Change email is disabled",
            ));
        }
        "/delete-user" => return Err(BlueprintError::new("HTTP_NOT_FOUND", "Not Found")),
        "/list-accounts" => return service.list_accounts(token, now).await,
        "/unlink-account" => {
            return service
                .unlink_account(token, super::text(&payload, "accountId")?, now)
                .await;
        }
        "/update-user" => {
            service.update_profile(token, &payload, now).await?;
            set_cookie(
                pending,
                &app.cookie.serialize(token, &app.secret, !transient),
            )?;
            if transient {
                set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
            }
            return Ok(json!({"status":true}));
        }
        _ => {}
    }

    if path == "/verify-password" {
        return service
            .verify_password(
                token,
                payload["password"]
                    .as_str()
                    .ok_or_else(internal)?
                    .to_owned(),
                transient,
                now,
            )
            .await;
    }
    let input: ChangePasswordInput = serde_json::from_value(payload).map_err(|_| internal())?;
    let (ip, agent) = super::request::client_metadata(headers);
    let changed = service
        .change_password(token, input, transient, &ip, &agent, now)
        .await?;
    if let Some(session) = &changed.session {
        set_cookie(
            pending,
            &app.cookie.serialize(
                session["token"].as_str().ok_or_else(internal)?,
                &app.secret,
                !transient,
            ),
        )?;
        if transient {
            set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
        }
    }
    Ok(changed.body())
}

// These features have no configured callbacks/enabled flags in the TS module.
fn configured_public(path: &str) -> Option<BlueprintError> {
    match path {
        "/request-password-reset" => Some(BlueprintError::new(
            "RESET_PASSWORD_DISABLED",
            "Reset password isn't enabled",
        )),
        "/send-verification-email" => Some(BlueprintError::new(
            "VERIFICATION_EMAIL_NOT_ENABLED",
            "Verification email isn't enabled",
        )),
        _ => None,
    }
}

async fn reset<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    payload: &Value,
    query: &std::collections::HashMap<String, Value>,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    if let Some(token) = query.get("token")
        && !token.is_string()
    {
        return Err(BlueprintError::new(
            "VALIDATION_ERROR",
            "[query.token] Invalid input: expected string, received array",
        ));
    }
    let token = payload["token"]
        .as_str()
        .filter(|token| !token.is_empty())
        .or_else(|| query.get("token").and_then(Value::as_str))
        .unwrap_or("");
    CredentialService::new(app.store.clone())
        .reset_password(token, super::text(payload, "newPassword")?.to_owned(), now)
        .await
}

pub(super) async fn public<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    payload: &Value,
    query: &std::collections::HashMap<String, Value>,
    now: DateTime<Utc>,
) -> Option<BlueprintResult<Value>> {
    if path == "/request-password-reset"
        && let Some(delivery) = &app.password_reset_delivery
    {
        let service = crate::accounts::PasswordResetService::new(
            app.store.clone(),
            app.auth_base_url(),
            app.policy.clone(),
            delivery.clone(),
        );
        return Some(match service {
            Err(error) => Err(error),
            Ok(mut service) => {
                if let Some(dispatcher) = &app.background_dispatcher {
                    service = service.with_background_dispatcher(dispatcher.clone());
                }
                service
                    .request(
                        payload["email"].as_str().unwrap_or(""),
                        payload["redirectTo"].as_str().unwrap_or(""),
                        now,
                    )
                    .await
            }
        });
    }
    if path == "/reset-password" {
        return Some(reset(app, payload, query, now).await);
    }
    configured_public(path).map(Err)
}

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Disables automatic signup sessions without disabling later password signin.
    #[must_use]
    pub fn with_signup_auto_sign_in(mut self, enabled: bool) -> Self {
        self.signup_auto_sign_in = enabled;
        self
    }
    /// Installs trusted email/password verification policy, independent of manual delivery.
    #[must_use]
    pub fn with_email_verification_policy(
        mut self,
        policy: crate::accounts::EmailVerificationPolicy,
    ) -> Self {
        self.email_verification_policy = policy;
        self
    }
    /// Enables trusted reset-link delivery; without this hook the route stays disabled.
    #[must_use]
    pub fn with_password_reset_delivery(
        mut self,
        delivery: std::sync::Arc<dyn crate::accounts::PasswordResetDelivery>,
    ) -> Self {
        self.password_reset_delivery = Some(delivery);
        self
    }
    /// Configures shared application-owned scheduling. Explicit OTP-specific
    /// dispatchers retain precedence, regardless of builder order.
    #[must_use]
    pub fn with_background_dispatcher(
        mut self,
        dispatcher: std::sync::Arc<dyn crate::background::AuthBackgroundDispatcher>,
    ) -> Self {
        if !self.otp_background_override
            && let Some(factor) = &self.two_factor
        {
            self.two_factor = Some(std::sync::Arc::new(
                factor
                    .copy_configuration()
                    .with_otp_background_dispatcher(dispatcher.clone()),
            ));
        }
        self.background_dispatcher = Some(dispatcher);
        self
    }
}
