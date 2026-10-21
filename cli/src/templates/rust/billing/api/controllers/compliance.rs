//! Subject data export and erasure.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn export_user_data(state: &AppState) -> Handler {
    state.billing.handler("exportuserdata")
}

pub fn erase_user_data(state: &AppState) -> Handler {
    state.billing.handler("eraseuserdata")
}
