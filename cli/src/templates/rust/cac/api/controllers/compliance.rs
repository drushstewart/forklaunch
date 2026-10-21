//! Subject data export and erasure.
//! Handlers come from `forklaunch-cac-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_cac_base::http::Handler;

pub fn export_user_data(state: &AppState) -> Handler {
    state.cac.handler("exportUserData")
}

pub fn erase_user_data(state: &AppState) -> Handler {
    state.cac.handler("eraseUserData")
}
