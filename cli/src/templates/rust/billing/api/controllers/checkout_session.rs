//! Checkout sessions.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn create_checkout_session(state: &AppState) -> Handler {
    state.billing.handler("createCheckoutSession")
}

pub fn get_checkout_session(state: &AppState) -> Handler {
    state.billing.handler("getCheckoutSession")
}

pub fn expire_checkout_session(state: &AppState) -> Handler {
    state.billing.handler("expireCheckoutSession")
}

pub fn handle_checkout_success(state: &AppState) -> Handler {
    state.billing.handler("handleCheckoutSuccess")
}

pub fn handle_checkout_failure(state: &AppState) -> Handler {
    state.billing.handler("handleCheckoutFailure")
}
