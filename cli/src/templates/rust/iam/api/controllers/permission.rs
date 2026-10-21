//! Directory permissions.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn create_permission(state: &AppState) -> Handler {
    state.iam.handler("createPermission")
}

pub fn get_permission(state: &AppState) -> Handler {
    state.iam.handler("getPermission")
}

pub fn update_permission(state: &AppState) -> Handler {
    state.iam.handler("updatePermission")
}

pub fn delete_permission(state: &AppState) -> Handler {
    state.iam.handler("deletePermission")
}

pub fn create_batch_permissions(state: &AppState) -> Handler {
    state.iam.handler("createBatchPermissions")
}

pub fn update_batch_permissions(state: &AppState) -> Handler {
    state.iam.handler("updateBatchPermissions")
}

pub fn delete_batch_permissions(state: &AppState) -> Handler {
    state.iam.handler("deleteBatchPermissions")
}

pub fn get_batch_permissions(state: &AppState) -> Handler {
    state.iam.handler("getBatchPermissions")
}
