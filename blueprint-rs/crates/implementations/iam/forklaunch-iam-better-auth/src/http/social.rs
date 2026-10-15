use super::{AuthHttp, internal, set_cookie};
use crate::{
    oauth::{google_accounts::GoogleAccounts, state::StateService},
    store::AuthStore,
};
use axum::http::{HeaderMap, HeaderValue, header};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

pub(super) async fn sign_in<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    payload: &Value,
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let provider = app
        .google
        .as_ref()
        .filter(|_| payload["provider"] == "google")
        .ok_or_else(|| BlueprintError::new("PROVIDER_NOT_FOUND", "Provider not found"))?;
    if let Some(id_token) = payload.get("idToken") {
        let token = id_token["token"].as_str().ok_or_else(internal)?;
        let mut profile = provider
            .verify(token, id_token["nonce"].as_str(), now)
            .await
            .ok_or_else(|| BlueprintError::new("INVALID_TOKEN", "Invalid token"))?;
        if !crate::keyring::verify::truthy(&profile["email_verified"]) {
            profile["email_verified"] = json!(false);
        }
        // Configured direct sign-in intentionally persists accessToken/idToken only;
        // refreshToken and expiresAt in the request are not copied by the TS handler.
        let mut tokens = json!({"idToken":token});
        if let Some(access) = id_token.get("accessToken") {
            tokens["accessToken"] = access.clone();
        }
        let (ip, agent) = super::request::client_metadata(headers);
        let grant = GoogleAccounts::new(app.store.clone())
            .with_session_policy(app.session_policy)
            .sign_in_verified(&profile, &tokens, &ip, &agent, now)
            .await?;
        set_cookie(
            pending,
            &app.cookie.serialize_with_policy(
                super::text(&grant.auth.session, "token")?,
                &app.secret,
                true,
                app.session_policy,
            ),
        )?;
        return Ok(
            json!({"redirect":false,"token":grant.auth.session["token"],"user":grant.auth.user}),
        );
    }
    authorization(app, payload, pending, now, None).await
}

pub(super) async fn authorization<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    payload: &Value,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
    link: Option<Value>,
) -> BlueprintResult<Value> {
    let provider = app
        .google
        .as_ref()
        .filter(|_| payload["provider"] == "google")
        .ok_or_else(|| BlueprintError::new("PROVIDER_NOT_FOUND", "Provider not found"))?;
    let auth_base = super::social_callback::auth_base(app);
    let mut data = payload.get("additionalData").cloned().unwrap_or(json!({}));
    // Reserved server state overrides caller additionalData, including absent fields.
    for field in [
        "callbackURL",
        "errorURL",
        "newUserURL",
        "link",
        "serverContext",
        "requestSignUp",
        "idTokenNonce",
    ] {
        data.as_object_mut().ok_or_else(internal)?.remove(field);
    }
    if let Some(link) = link {
        data["link"] = link;
    }
    let default_callback = url::Url::parse(&app.base_url)
        .map_err(|_| internal())?
        .origin()
        .ascii_serialization();
    data["callbackURL"] = json!(
        payload["callbackURL"]
            .as_str()
            .filter(|value| !value.is_empty())
            .unwrap_or(&default_callback)
    );
    for (source, target) in [
        ("errorCallbackURL", "errorURL"),
        ("newUserCallbackURL", "newUserURL"),
        ("requestSignUp", "requestSignUp"),
    ] {
        if let Some(value) = payload.get(source) {
            data[target] = value.clone();
        }
    }
    let grant = StateService::new(app.store.clone(), app.secret.to_vec(), app.cookie.secure)?
        .generate(data, now)
        .await?;
    set_cookie(pending, &grant.set_cookie)?;
    let scopes = payload["scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let params = payload["additionalParams"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
        .collect::<Vec<_>>();
    let url = provider
        .authorization(format!("{auth_base}/callback/google"))
        .authorization_url(
            &grant.state,
            &grant.code_verifier,
            &scopes,
            payload["loginHint"].as_str(),
            &params,
        )?;
    let redirect = payload["disableRedirect"] != true;
    if redirect {
        pending.insert(
            header::LOCATION,
            HeaderValue::from_str(&url).map_err(|_| internal())?,
        );
    }
    Ok(json!({"url":url,"redirect":redirect}))
}

pub(super) fn validate(payload: Option<&Value>) -> BlueprintResult<()> {
    let mut errors = Vec::new();
    object(payload, "body", &mut errors);
    if payload.is_some_and(Value::is_object) {
        let body = payload.expect("checked body");
        for field in ["callbackURL", "newUserCallbackURL", "errorCallbackURL"] {
            scalar(
                body.get(field),
                &format!("body.{field}"),
                "string",
                false,
                &mut errors,
            );
        }
        if !body["provider"].is_string() {
            errors.push("[body.provider] Invalid input".into());
        }
        scalar(
            body.get("disableRedirect"),
            "body.disableRedirect",
            "boolean",
            false,
            &mut errors,
        );
        validate_id_token(body, &mut errors);
        if let Some(scopes) = body.get("scopes") {
            if let Some(scopes) = scopes.as_array() {
                for (i, scope) in scopes.iter().enumerate() {
                    scalar(
                        Some(scope),
                        &format!("body.scopes.{i}"),
                        "string",
                        true,
                        &mut errors,
                    );
                }
            } else {
                invalid(Some(scopes), "body.scopes", "array", &mut errors);
            }
        }
        scalar(
            body.get("requestSignUp"),
            "body.requestSignUp",
            "boolean",
            false,
            &mut errors,
        );
        scalar(
            body.get("loginHint"),
            "body.loginHint",
            "string",
            false,
            &mut errors,
        );
        validate_additional(body, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
pub(super) fn object(value: Option<&Value>, path: &str, errors: &mut Vec<String>) -> bool {
    if value.is_some_and(Value::is_object) {
        true
    } else {
        invalid(value, path, "object", errors);
        false
    }
}
pub(super) fn scalar(
    value: Option<&Value>,
    path: &str,
    kind: &str,
    required: bool,
    errors: &mut Vec<String>,
) {
    if value.is_none() && !required {
        return;
    }
    let valid = value.is_some_and(|value| match kind {
        "boolean" => value.is_boolean(),
        "number" => value.as_f64().is_some_and(f64::is_finite),
        _ => value.is_string(),
    });
    if !valid {
        invalid(value, path, kind, errors);
    }
}
pub(super) fn invalid(value: Option<&Value>, path: &str, kind: &str, errors: &mut Vec<String>) {
    errors.push(format!(
        "[{path}] Invalid input: expected {kind}, received {}",
        super::request::received(value)
    ));
}

fn validate_id_token(body: &Value, errors: &mut Vec<String>) {
    if let Some(token) = body.get("idToken")
        && object(Some(token), "body.idToken", errors)
    {
        for (field, kind, required) in [
            ("token", "string", true),
            ("nonce", "string", false),
            ("accessToken", "string", false),
            ("refreshToken", "string", false),
            ("expiresAt", "number", false),
        ] {
            scalar(
                token.get(field),
                &format!("body.idToken.{field}"),
                kind,
                required,
                errors,
            );
        }
        if let Some(user) = token.get("user")
            && object(Some(user), "body.idToken.user", errors)
        {
            if let Some(name) = user.get("name")
                && object(Some(name), "body.idToken.user.name", errors)
            {
                for field in ["firstName", "lastName"] {
                    scalar(
                        name.get(field),
                        &format!("body.idToken.user.name.{field}"),
                        "string",
                        false,
                        errors,
                    );
                }
            }
            scalar(
                user.get("email"),
                "body.idToken.user.email",
                "string",
                false,
                errors,
            );
        }
    }
}

pub(super) fn validate_additional(body: &Value, errors: &mut Vec<String>) {
    if let Some(params) = body.get("additionalParams") {
        if let Some(params) = params.as_object() {
            let before = errors.len();
            for (key, value) in params {
                scalar(
                    Some(value),
                    &format!("body.additionalParams.{key}"),
                    "string",
                    true,
                    errors,
                );
            }
            if errors.len() == before
                && params
                    .keys()
                    .any(|key| crate::oauth::google_authorize::RESERVED.contains(&key.as_str()))
            {
                errors.push("[body.additionalParams] additionalParams cannot include reserved OAuth parameters: state, client_id, redirect_uri, response_type, code_challenge, code_challenge_method, nonce, scope".into());
            }
        } else {
            invalid(Some(params), "body.additionalParams", "record", errors);
        }
    }
    if let Some(data) = body.get("additionalData")
        && !data.is_object()
    {
        invalid(Some(data), "body.additionalData", "record", errors);
    }
}
