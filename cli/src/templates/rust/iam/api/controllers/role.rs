//! Directory roles.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn create_role(state: &AppState) -> Handler {
    state.iam.handler("createRole")
}

pub fn get_role(state: &AppState) -> Handler {
    state.iam.handler("getRole")
}

pub fn update_role(state: &AppState) -> Handler {
    state.iam.handler("updateRole")
}

pub fn delete_role(state: &AppState) -> Handler {
    state.iam.handler("deleteRole")
}

pub fn create_batch_roles(state: &AppState) -> Handler {
    state.iam.handler("createBatchRoles")
}

pub fn update_batch_roles(state: &AppState) -> Handler {
    state.iam.handler("updateBatchRoles")
}

pub fn delete_batch_roles(state: &AppState) -> Handler {
    state.iam.handler("deleteBatchRoles")
}

pub fn get_batch_roles(state: &AppState) -> Handler {
    state.iam.handler("getBatchRoles")
}
