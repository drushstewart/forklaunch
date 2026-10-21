//! Payment links.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn create_payment_link(state: &AppState) -> Handler {
    state.billing.handler("createPaymentLink")
}

pub fn get_payment_link(state: &AppState) -> Handler {
    state.billing.handler("getPaymentLink")
}

pub fn update_payment_link(state: &AppState) -> Handler {
    state.billing.handler("updatePaymentLink")
}

pub fn expire_payment_link(state: &AppState) -> Handler {
    state.billing.handler("expirePaymentLink")
}

pub fn handle_payment_success(state: &AppState) -> Handler {
    state.billing.handler("handlePaymentSuccess")
}

pub fn handle_payment_failure(state: &AppState) -> Handler {
    state.billing.handler("handlePaymentFailure")
}

pub fn list_payment_links(state: &AppState) -> Handler {
    state.billing.handler("listPaymentLinks")
}
