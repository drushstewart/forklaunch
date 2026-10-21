//! Directory users and their surfaced grants.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn create_user(state: &AppState) -> Handler {
    state.iam.handler("createUser")
}

pub fn get_user(state: &AppState) -> Handler {
    state.iam.handler("getUser")
}

pub fn update_user(state: &AppState) -> Handler {
    state.iam.handler("updateUser")
}

pub fn delete_user(state: &AppState) -> Handler {
    state.iam.handler("deleteUser")
}

pub fn create_batch_users(state: &AppState) -> Handler {
    state.iam.handler("createBatchUsers")
}

pub fn update_batch_users(state: &AppState) -> Handler {
    state.iam.handler("updateBatchUsers")
}

pub fn delete_batch_users(state: &AppState) -> Handler {
    state.iam.handler("deleteBatchUsers")
}

pub fn get_batch_users(state: &AppState) -> Handler {
    state.iam.handler("getBatchUsers")
}

pub fn surface_user_roles(state: &AppState) -> Handler {
    state.iam.handler("surfaceUserRoles")
}

pub fn surface_user_permissions(state: &AppState) -> Handler {
    state.iam.handler("surfaceUserPermissions")
}
