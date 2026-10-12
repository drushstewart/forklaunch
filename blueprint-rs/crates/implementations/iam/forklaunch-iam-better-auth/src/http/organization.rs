//! Configured organization HTTP bodies, queries and session cookie effects.
use super::{AuthHttp, internal, set_cookie};
use crate::{organizations::OrganizationService, store::AuthStore};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::collections::HashMap;

mod members;
mod mutation_validation;
mod teams;
mod validation;
pub(super) use validation::{validate_body, validate_query};

pub(super) async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    payload: &Value,
    query: &HashMap<String, Value>,
    session: (&str, bool),
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let service = OrganizationService::new(app.store.clone());
    let (token, transient) = session;
    if let Some(body) = members::handle(&service, path, payload, query, session, now).await? {
        return Ok(body);
    }
    if let Some(body) = teams::handle(&service, path, payload, query, session, now).await? {
        return Ok(body);
    }
    match path {
        "/organization/update" => service.update(token, transient, payload, now).await,
        "/organization/delete" => {
            service
                .delete(
                    token,
                    transient,
                    payload["organizationId"].as_str().ok_or_else(internal)?,
                    now,
                )
                .await
        }
        "/organization/has-permission" => {
            service.has_permission(token, transient, payload, now).await
        }
        "/organization/get-organization" => {
            service
                .metadata(
                    token,
                    transient,
                    query.get("organizationId").and_then(Value::as_str),
                    query.get("organizationSlug").and_then(Value::as_str),
                    now,
                )
                .await
        }
        "/organization/create" => service.create(token, transient, payload, now).await,
        "/organization/check-slug" => {
            service
                .check_slug(
                    token,
                    transient,
                    payload["slug"].as_str().ok_or_else(internal)?,
                    now,
                )
                .await
        }
        "/organization/list" => service.list(token, transient, now).await,
        "/organization/get-full-organization" => {
            service
                .full(
                    token,
                    transient,
                    query.get("organizationId").and_then(Value::as_str),
                    query.get("organizationSlug").and_then(Value::as_str),
                    query
                        .get("membersLimit")
                        .and_then(Value::as_str)
                        .and_then(validation::integer),
                    now,
                )
                .await
        }
        "/organization/set-active" | "/organization/set-active-team" => {
            let result = if path == "/organization/set-active-team" {
                service
                    .set_active_team(token, transient, payload, now)
                    .await?
            } else {
                service.set_active(token, transient, payload, now).await?
            };
            if result.cookie {
                set_cookie(
                    pending,
                    &app.cookie.serialize(token, &app.secret, !transient),
                )?;
                if transient {
                    set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
                }
            }
            Ok(result.body)
        }
        _ => Err(internal()),
    }
}
