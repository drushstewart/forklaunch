use super::{
    AuthHttp, internal,
    social::{invalid, object, scalar},
};
use crate::{oauth::google_accounts::GoogleAccounts, sessions::SessionView, store::AuthStore};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

pub(super) async fn link<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    payload: &Value,
    view: &SessionView,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let provider = app
        .google
        .as_ref()
        .filter(|_| payload["provider"] == "google")
        .ok_or_else(|| BlueprintError::new("PROVIDER_NOT_FOUND", "Provider not found"))?;
    if let Some(id_token) = payload.get("idToken") {
        let token = super::text(id_token, "token")?;
        let profile = provider
            .verify(token, id_token["nonce"].as_str(), now)
            .await
            .ok_or_else(|| BlueprintError::new("INVALID_TOKEN", "Invalid token"))?;
        let mut tokens = json!({"idToken":token});
        for field in ["accessToken", "refreshToken"] {
            if let Some(value) = id_token.get(field) {
                tokens[field] = value.clone();
            }
        }
        GoogleAccounts::new(app.store.clone())
            .link_direct_verified(&profile, &tokens, super::text(&view.user, "id")?, now)
            .await?;
        return Ok(json!({"url":"","status":true,"redirect":false}));
    }
    let mut payload = payload.clone();
    // This unknown field is stripped by the link schema before state generation.
    payload
        .as_object_mut()
        .ok_or_else(internal)?
        .remove("newUserCallbackURL");
    super::social::authorization(
        app,
        &payload,
        pending,
        now,
        Some(json!({"userId":view.user["id"],"email":view.user["email"]})),
    )
    .await
}
pub(super) fn validate(payload: Option<&Value>) -> BlueprintResult<()> {
    let mut errors = Vec::new();
    if object(payload, "body", &mut errors) {
        let body = payload.expect("checked body");
        scalar(
            body.get("callbackURL"),
            "body.callbackURL",
            "string",
            false,
            &mut errors,
        );
        if !body["provider"].is_string() {
            errors.push("[body.provider] Invalid input".into());
        }
        if let Some(token) = body.get("idToken")
            && object(Some(token), "body.idToken", &mut errors)
        {
            for (field, required) in [
                ("token", true),
                ("nonce", false),
                ("accessToken", false),
                ("refreshToken", false),
            ] {
                scalar(
                    token.get(field),
                    &format!("body.idToken.{field}"),
                    "string",
                    required,
                    &mut errors,
                );
            }
        }
        scalar(
            body.get("requestSignUp"),
            "body.requestSignUp",
            "boolean",
            false,
            &mut errors,
        );
        if let Some(scopes) = body.get("scopes") {
            if let Some(scopes) = scopes.as_array() {
                for (i, value) in scopes.iter().enumerate() {
                    scalar(
                        Some(value),
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
        for (field, kind) in [
            ("errorCallbackURL", "string"),
            ("disableRedirect", "boolean"),
            ("loginHint", "string"),
        ] {
            scalar(
                body.get(field),
                &format!("body.{field}"),
                kind,
                false,
                &mut errors,
            );
        }
        super::social::validate_additional(body, &mut errors);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")))
    }
}
