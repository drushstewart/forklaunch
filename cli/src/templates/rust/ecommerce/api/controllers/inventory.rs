//! Inventory and stock.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_inventory(state: &AppState) -> Handler {
    state.ecommerce.handler("createInventory")
}

pub fn get_inventory(state: &AppState) -> Handler {
    state.ecommerce.handler("getInventory")
}

pub fn adjust_stock(state: &AppState) -> Handler {
    state.ecommerce.handler("adjustStock")
}

pub fn check_stock(state: &AppState) -> Handler {
    state.ecommerce.handler("checkStock")
}
