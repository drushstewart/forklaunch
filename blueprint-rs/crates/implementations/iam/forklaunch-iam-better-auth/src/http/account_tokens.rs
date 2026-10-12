//! Explicit local account selection, provider refresh and account information.
use super::{AuthHttp, internal};
use crate::{
    keyring::verify::truthy,
    store::{AuthStore, AuthTransaction, Model},
};
use chrono::{DateTime, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Value, json};

pub(super) fn validate(input: Option<&Value>, prefix: &str) -> BlueprintResult<()> {
    let invalid = || BlueprintError::new("VALIDATION_ERROR", format!("[{prefix}] Invalid input"));
    let object = input.and_then(Value::as_object).ok_or_else(invalid)?;
    let user_valid = object.get("userId").is_none_or(Value::is_string);
    let by_id = user_valid && object.get("accountId").is_some_and(Value::is_string);
    let by_cookie = user_valid && object.get("useAccountCookie") == Some(&json!(true));
    let extras = |selected| {
        let mut fields = object
            .keys()
            .filter(|field| field.as_str() != selected && field.as_str() != "userId")
            .cloned()
            .collect::<Vec<_>>();
        fields.sort_by_key(|key| {
            key.parse::<u32>()
                .ok()
                .filter(|index| *index != u32::MAX && index.to_string() == *key)
                .map_or(u64::MAX, u64::from)
        });
        fields
    };
    if by_id && extras("accountId").is_empty() || by_cookie && extras("useAccountCookie").is_empty()
    {
        return Ok(());
    }
    let fields = match (by_id, by_cookie) {
        (true, false) => extras("accountId"),
        (false, true) => extras("useAccountCookie"),
        _ => return Err(invalid()),
    };
    let quoted = fields
        .iter()
        .map(|field| crate::ecmascript::stringify(&json!(field)))
        .collect::<Vec<_>>()
        .join(", ");
    Err(BlueprintError::new(
        "VALIDATION_ERROR",
        format!(
            "[{prefix}] Unrecognized {}: {quoted}",
            if fields.len() == 1 { "key" } else { "keys" }
        ),
    ))
}
pub(super) async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    selection: &Value,
    user_id: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    // The configured account cookie feature is disabled; cookie selection never
    // silently falls back to another account or to a provider's subject ID.
    let account_id = selection["accountId"].as_str().ok_or_else(not_found)?;
    let mut tx = app.store.begin(false).await?;
    let account = tx
        .get(Model::Account, account_id)
        .await?
        .filter(|row| row["userId"] == user_id)
        .ok_or_else(not_found)?;
    tx.commit().await?;
    let provider_id = super::text(&account, "providerId")?;
    let Some(provider) = app.google.as_ref().filter(|_| provider_id == "google") else {
        return Err(if path == "/account-info" {
            error(
                "PROVIDER_NOT_CONFIGURED",
                "Account is not associated with a configured social provider.",
            )
        } else {
            error(
                "PROVIDER_NOT_SUPPORTED",
                &format!("Provider {provider_id} is not supported."),
            )
        });
    };
    let refresh = path == "/refresh-token";
    if refresh && !truthy(&account["refreshToken"]) {
        return Err(error("REFRESH_TOKEN_NOT_FOUND", "Refresh token not found"));
    }
    let expiring = account["accessTokenExpiresAt"]
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|expires| {
            expires
                .timestamp_millis()
                .saturating_sub(now.timestamp_millis())
                < 5000
        });
    let tokens = if refresh || truthy(&account["refreshToken"]) && expiring {
        let refresh_token = crate::ecmascript::string(account.get("refreshToken"))
            .ok_or_else(|| refresh_error(refresh))?;
        let tokens = provider
            .refresh_token(&refresh_token, now)
            .await
            .map_err(|_| refresh_error(refresh))?;
        persist(app, &account, &tokens, user_id, now)
            .await
            .map_err(|_| refresh_error(refresh))?;
        Some(tokens)
    } else {
        None
    };
    if refresh {
        return Ok(refresh_response(
            &account,
            tokens.as_ref().ok_or_else(internal)?,
        ));
    }
    let result = access_response(&account, tokens.as_ref())?;
    if path != "/account-info" {
        return Ok(result);
    }
    if !truthy(&result["accessToken"]) {
        return Err(error("ACCESS_TOKEN_NOT_FOUND", "Access token not found"));
    }
    if !truthy(&result["idToken"]) {
        return Err(error("FAILED_TO_GET_USER_INFO", "Failed to get user info"));
    }
    let token = result["idToken"]
        .as_str()
        .ok_or_else(|| error("HTTP_INTERNAL_EMPTY", ""))?;
    let profile = crate::oauth::google_tokens::decode_profile(token)
        .ok_or_else(|| error("HTTP_INTERNAL_EMPTY", ""))?;
    let mut user = json!({"firstName":profile["given_name"].as_str().unwrap_or(""),"lastName":profile["family_name"].as_str().unwrap_or("")});
    for (claim, field) in [
        ("name", "name"),
        ("email", "email"),
        ("picture", "image"),
        ("email_verified", "emailVerified"),
    ] {
        if let Some(value) = profile.get(claim) {
            user[field] = value.clone();
        }
    }
    Ok(
        json!({"user":user,"data":profile,"account":{"id":account["id"],"providerId":provider_id,"accountId":account["accountId"]}}),
    )
}
async fn persist<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    original: &Value,
    tokens: &Value,
    user_id: &str,
    now: DateTime<Utc>,
) -> BlueprintResult<()> {
    let mut tx = app.store.begin(true).await?;
    let mut account = tx
        .get(Model::Account, super::text(original, "id")?)
        .await?
        .ok_or_else(not_found)?;
    if account["userId"] != user_id
        || account["accountId"] != original["accountId"]
        || account["providerId"] != original["providerId"]
        || account["refreshToken"] != original["refreshToken"]
    {
        return Err(not_found());
    }
    for field in [
        "accessToken",
        "accessTokenExpiresAt",
        "refreshTokenExpiresAt",
    ] {
        if let Some(value) = tokens.get(field) {
            account[field] = value.clone();
        }
    }
    for field in ["refreshToken", "idToken"] {
        if let Some(value) = tokens.get(field).filter(|value| truthy(value)) {
            account[field] = value.clone();
        }
    }
    account["updatedAt"] = json!(now.to_rfc3339_opts(SecondsFormat::Millis, true));
    tx.replace(Model::Account, account).await?;
    tx.commit().await
}
fn access_response(account: &Value, tokens: Option<&Value>) -> BlueprintResult<Value> {
    let mut response = json!({"accessToken":tokens.and_then(|tokens| tokens.get("accessToken")).filter(|value| !value.is_null())
        .or_else(|| account.get("accessToken").filter(|value| !value.is_null())).cloned().unwrap_or(json!("")),
        "scopes":stored_scopes(account)?});
    if let Some(expires) = tokens
        .and_then(|tokens| tokens.get("accessTokenExpiresAt"))
        .or_else(|| {
            account
                .get("accessTokenExpiresAt")
                .filter(|value| truthy(value))
        })
    {
        response["accessTokenExpiresAt"] = expires.clone();
    }
    if let Some(token) = tokens
        .and_then(|tokens| tokens.get("idToken"))
        .filter(|value| !value.is_null())
        .or_else(|| account.get("idToken").filter(|value| !value.is_null()))
    {
        response["idToken"] = token.clone();
    }
    Ok(response)
}
fn refresh_response(account: &Value, tokens: &Value) -> Value {
    let mut response = json!({"providerId":account["providerId"],"accountId":account["id"],
        "refreshToken":tokens.get("refreshToken").filter(|value| !value.is_null()).unwrap_or(&account["refreshToken"])});
    for field in ["accessToken", "accessTokenExpiresAt"] {
        if let Some(value) = tokens.get(field) {
            response[field] = value.clone();
        }
    }
    if let Some(value) = tokens
        .get("refreshTokenExpiresAt")
        .or_else(|| account.get("refreshTokenExpiresAt"))
    {
        response["refreshTokenExpiresAt"] = value.clone();
    }
    if let Some(value) = tokens
        .get("idToken")
        .filter(|value| truthy(value))
        .or_else(|| account.get("idToken"))
    {
        response["idToken"] = value.clone();
    }
    if let Some(value) = account.get("scope") {
        response["scope"] = value.clone();
    }
    response
}
fn stored_scopes(account: &Value) -> BlueprintResult<Vec<String>> {
    if truthy(&account["scope"]) && !account["scope"].is_string() {
        return Err(refresh_error(false));
    }
    Ok(account["scope"]
        .as_str()
        .into_iter()
        .flat_map(|scope| scope.split(','))
        .map(|scope| {
            scope.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}')
        })
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect())
}
fn not_found() -> BlueprintError {
    error("ACCOUNT_NOT_FOUND", "Account not found")
}
fn refresh_error(explicit: bool) -> BlueprintError {
    if explicit {
        error(
            "FAILED_TO_REFRESH_ACCESS_TOKEN",
            "Failed to refresh access token",
        )
    } else {
        error(
            "FAILED_TO_GET_ACCESS_TOKEN",
            "Failed to get a valid access token",
        )
    }
}
fn error(code: &str, message: &str) -> BlueprintError {
    BlueprintError::new(code, message)
}
