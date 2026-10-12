use super::{AuthHttp, internal, set_cookie};
use crate::{
    accounts::{ChangePasswordInput, CredentialService, SigninInput, SignupInput, validate_length},
    store::AuthStore,
};
use axum::http::{HeaderMap, HeaderValue, header};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

pub(super) async fn login<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    mut payload: Value,
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let service = CredentialService::new(app.store.clone());
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
        let input: SignupInput = serde_json::from_value(payload).map_err(|_| internal())?;
        service
            .sign_up_with_fields(input, profile, &ip, &agent, now)
            .await?
    } else {
        let input: SigninInput = serde_json::from_value(payload).map_err(|_| internal())?;
        service.sign_in(input, &ip, &agent, now).await?
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
    Ok(if path == "/sign-up/email" {
        grant.signup_body()
    } else {
        grant.signin_body()
    })
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
pub(super) fn configured_public(path: &str) -> Option<BlueprintError> {
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
