//! Subject data export and erasure.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn export_user_data(state: &AppState) -> Handler {
    state.iam.handler("exportuserdata")
}

pub fn erase_user_data(state: &AppState) -> Handler {
    state.iam.handler("eraseuserdata")
}
