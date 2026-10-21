//! Health check.
//! Handlers come from `forklaunch-iam-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_iam_base::http::Handler;

pub fn get_health(state: &AppState) -> Handler {
    state.iam.handler("getHealth")
}
