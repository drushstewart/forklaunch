//! Configured organization HTTP bodies, queries and session cookie effects.
use super::{AuthHttp, internal, set_cookie};
use crate::{organizations::OrganizationService, store::AuthStore};
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::collections::HashMap;

pub(super) mod invitations;
mod members;
mod mutation_validation;
mod roles;
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
    if let Some(body) = roles::handle(&service, path, payload, query, session, now).await? {
        return Ok(body);
    }
    if path == "/organization/accept-invitation" {
        let result = service
            .accept_invitation(
                token,
                transient,
                payload["invitationId"].as_str().unwrap_or(""),
                now,
            )
            .await?;
        return selected(app, pending, session, result);
    }
    if let Some(body) = invitations::handle(&service, path, payload, query, session, now).await? {
        return Ok(body);
    }
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
            selected(app, pending, session, result)
        }
        _ => Err(internal()),
    }
}

pub(super) fn strip_record_keys(path: &str, payload: &mut Value) {
    // Zod record construction skips the own __proto__ key at this level.
    let metadata = match path {
        "/organization/create" => payload.get_mut("metadata"),
        "/organization/create-role" => payload.get_mut("permission"),
        "/organization/update-role" => payload
            .get_mut("data")
            .and_then(|data| data.get_mut("permission")),
        "/organization/update" => payload
            .get_mut("data")
            .and_then(|data| data.get_mut("metadata")),
        _ => None,
    };
    if let Some(Value::Object(metadata)) = metadata {
        metadata.shift_remove("__proto__");
    }
}

pub(super) fn routes<S: AuthStore + 'static>() -> axum::Router<std::sync::Arc<AuthHttp<S>>> {
    use super::{dispatch, not_found};
    use axum::{
        Router,
        routing::{get, post},
    };
    Router::new()
        .route("/organization/create-role", post(dispatch::<S>))
        .route("/organization/update-role", post(dispatch::<S>))
        .route("/organization/delete-role", post(dispatch::<S>))
        .route(
            "/organization/list-roles",
            get(dispatch::<S>).head(not_found),
        )
        .route("/organization/get-role", get(dispatch::<S>).head(not_found))
        .route("/organization/invite-member", post(dispatch::<S>))
        .route("/organization/accept-invitation", post(dispatch::<S>))
        .route("/organization/reject-invitation", post(dispatch::<S>))
        .route("/organization/cancel-invitation", post(dispatch::<S>))
        .route(
            "/organization/get-invitation",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-invitations",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-user-invitations",
            get(dispatch::<S>).head(not_found),
        )
        .route("/organization/create", post(dispatch::<S>))
        .route(
            "/organization/get-active-member",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/get-active-member-role",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-members",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-teams",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-user-teams",
            get(dispatch::<S>).head(not_found),
        )
        .route(
            "/organization/list-team-members",
            get(dispatch::<S>).head(not_found),
        )
        .route("/organization/set-active-team", post(dispatch::<S>))
        .route("/organization/create-team", post(dispatch::<S>))
        .route("/organization/update-team", post(dispatch::<S>))
        .route("/organization/remove-team", post(dispatch::<S>))
        .route("/organization/add-team-member", post(dispatch::<S>))
        .route("/organization/remove-team-member", post(dispatch::<S>))
        .route("/organization/leave", post(dispatch::<S>))
        .route("/organization/update-member-role", post(dispatch::<S>))
        .route("/organization/remove-member", post(dispatch::<S>))
        .route("/organization/update", post(dispatch::<S>))
        .route("/organization/delete", post(dispatch::<S>))
        .route("/organization/has-permission", post(dispatch::<S>))
        .route(
            "/organization/get-organization",
            get(dispatch::<S>).head(not_found),
        )
        .route("/organization/check-slug", post(dispatch::<S>))
        .route("/organization/set-active", post(dispatch::<S>))
        .route("/organization/list", get(dispatch::<S>).head(not_found))
        .route(
            "/organization/get-full-organization",
            get(dispatch::<S>).head(not_found),
        )
}

fn selected<S: AuthStore>(
    app: &AuthHttp<S>,
    pending: &mut HeaderMap,
    session: (&str, bool),
    result: crate::organizations::OrganizationSelection,
) -> BlueprintResult<Value> {
    let (token, transient) = session;
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
