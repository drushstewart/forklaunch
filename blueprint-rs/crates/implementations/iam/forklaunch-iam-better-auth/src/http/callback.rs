//! GET token callbacks validate trusted destinations without mutation CSRF checks.
use super::{AuthHttp, failure, internal, text};
use crate::store::{AuthStore, AuthTransaction, Model};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use url::Url;

pub(super) async fn reset<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    let token = request
        .uri()
        .path()
        .strip_prefix("/reset-password/")
        .unwrap_or("");
    match reset_response(&app, token, request.uri().query().unwrap_or("")).await {
        Ok(response) => response,
        Err(error) => failure(&error),
    }
}
async fn reset_response<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    token: &str,
    query: &str,
) -> BlueprintResult<Response> {
    let query = super::request::parse_query(query);
    let callback = query_string(&query, "callbackURL", true)?.unwrap_or("");
    trusted(app, callback)?;
    let mut base = Url::parse(&app.base_url).map_err(|_| internal())?;
    if base.path().trim_matches('/').is_empty() {
        base.set_path("/api/auth");
    }
    let mut valid = false;
    if !token.is_empty() && !callback.is_empty() {
        let mut tx = app.store.begin(true).await?;
        let mut latest: Option<(DateTime<Utc>, Value)> = None;
        for row in tx.list(Model::Verification).await? {
            if row["identifier"].as_str() == Some(&format!("reset-password:{token}")) {
                let created = time(&row, "createdAt")?;
                if latest.as_ref().is_none_or(|(best, _)| created > *best) {
                    latest = Some((created, row.clone()));
                }
            }
            if time(&row, "expiresAt")? < (app.clock)() {
                tx.delete(Model::Verification, text(&row, "id")?).await?;
            }
        }
        tx.commit().await?;
        if let Some((_, row)) = latest {
            valid = time(&row, "expiresAt")? >= (app.clock)();
        }
    }
    let mut destination = if callback.is_empty() {
        Url::parse(&format!("{base}/error")).map_err(|_| internal())?
    } else {
        base.join(callback).map_err(|_| internal())?
    };
    set_query(
        &mut destination,
        if valid { "token" } else { "error" },
        if valid { token } else { "INVALID_TOKEN" },
    );
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::FOUND;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(destination.as_str()).map_err(|_| internal())?,
    );
    Ok(response)
}
pub(super) async fn delete<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
) -> Response {
    let query = super::request::parse_query(request.uri().query().unwrap_or(""));
    let result = (|| {
        let errors: Vec<_> = [("token", true), ("callbackURL", false)]
            .into_iter()
            .filter_map(|(field, required)| {
                query_string(&query, field, required)
                    .err()
                    .map(|error| error.message)
            })
            .collect();
        if !errors.is_empty() {
            return Err(BlueprintError::new("VALIDATION_ERROR", errors.join("; ")));
        }
        if let Some(callback) = query_string(&query, "callbackURL", false)? {
            trusted(&app, callback)?;
        }
        Ok::<_, BlueprintError>(())
    })();
    match result {
        Err(error) => failure(&error),
        Ok(()) => (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({"message":"Not found","code":"NOT_FOUND"})),
        )
            .into_response(),
    }
}
use axum::response::IntoResponse;
fn query_string<'a>(
    query: &'a HashMap<String, Value>,
    field: &str,
    required: bool,
) -> BlueprintResult<Option<&'a str>> {
    match query.get(field) {
        Some(Value::String(value)) => Ok(Some(value)),
        None if !required => Ok(None),
        value => Err(BlueprintError::new(
            "VALIDATION_ERROR",
            format!(
                "[query.{field}] Invalid input: expected string, received {}",
                if value.is_none() {
                    "undefined"
                } else {
                    "array"
                }
            ),
        )),
    }
}
fn trusted<S>(app: &AuthHttp<S>, callback: &str) -> BlueprintResult<()> {
    if !callback.is_empty() && !app.policy.trusted(callback, true) {
        return Err(BlueprintError::new(
            "INVALID_CALLBACK_URL",
            "Invalid callbackURL",
        ));
    }
    Ok(())
}
fn time(row: &Value, field: &str) -> BlueprintResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text(row, field)?)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|_| internal())
}
fn set_query(url: &mut Url, name: &str, value: &str) {
    let mut found = false;
    let mut pairs = Vec::new();
    for (key, old) in url.query_pairs() {
        if key == name {
            if !found {
                pairs.push((name.to_owned(), value.to_owned()));
                found = true;
            }
        } else {
            pairs.push((key.into_owned(), old.into_owned()));
        }
    }
    if !found {
        pairs.push((name.to_owned(), value.to_owned()));
    }
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(pairs);
}
