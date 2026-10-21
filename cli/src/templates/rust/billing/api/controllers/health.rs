//! Health check.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn get_health(state: &AppState) -> Handler {
    state.billing.handler("getHealth")
}
