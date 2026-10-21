//! Directory organizations.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn create_organization(state: &AppState) -> Handler {
    state.iam.handler("createOrganization")
}

pub fn get_organization(state: &AppState) -> Handler {
    state.iam.handler("getOrganization")
}

pub fn update_organization(state: &AppState) -> Handler {
    state.iam.handler("updateOrganization")
}

pub fn delete_organization(state: &AppState) -> Handler {
    state.iam.handler("deleteOrganization")
}
