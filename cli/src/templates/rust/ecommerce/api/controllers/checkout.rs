//! Checkout with tax and shipping.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn checkout(state: &AppState) -> Handler {
    state.ecommerce.handler("checkout")
}
