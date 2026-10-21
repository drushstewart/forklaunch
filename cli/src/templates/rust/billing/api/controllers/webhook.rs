{{#stripe}}//! Stripe and managed platform event callbacks.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn handle_webhook_event(state: &AppState) -> Handler {
    state.billing.handler("handlewebhookevent")
}

pub fn receive_platform_event(state: &AppState) -> Handler {
    state.billing.handler("receiveplatformevent")
}
{{/stripe}}