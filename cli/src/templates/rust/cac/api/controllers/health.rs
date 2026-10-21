//! Health check.
//! Handlers come from `forklaunch-cac-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_cac_base::http::Handler;

pub fn get_health(state: &AppState) -> Handler {
    state.cac.handler("getHealth")
}
