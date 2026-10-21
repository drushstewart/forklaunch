//! Orders and their transitions.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_order(state: &AppState) -> Handler {
    state.ecommerce.handler("createOrder")
}

pub fn list_orders(state: &AppState) -> Handler {
    state.ecommerce.handler("listOrders")
}

pub fn get_order(state: &AppState) -> Handler {
    state.ecommerce.handler("getOrder")
}

pub fn transition_order(state: &AppState) -> Handler {
    state.ecommerce.handler("transitionOrder")
}
