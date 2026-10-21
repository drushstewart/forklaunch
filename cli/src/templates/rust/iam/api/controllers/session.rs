//! Credential login, opaque session and session-scoped lookup.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn authenticate(state: &AppState) -> Handler {
    state.iam.handler("authenticate")
}

pub fn logout(state: &AppState) -> Handler {
    state.iam.handler("logout")
}

pub fn get_session_user(state: &AppState) -> Handler {
    state.iam.handler("getSessionUser")
}

pub fn list_users(state: &AppState) -> Handler {
    state.iam.handler("listUsers")
}

pub fn list_user_permissions(state: &AppState) -> Handler {
    state.iam.handler("listUserPermissions")
}

pub fn list_organizations(state: &AppState) -> Handler {
    state.iam.handler("listOrganizations")
}

pub fn get_auth_methods(state: &AppState) -> Handler {
    state.iam.handler("getauthmethods")
}
