//! Provider payments.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_payment(state: &AppState) -> Handler {
    state.ecommerce.handler("createPayment")
}

pub fn get_payment(state: &AppState) -> Handler {
    state.ecommerce.handler("getPayment")
}
