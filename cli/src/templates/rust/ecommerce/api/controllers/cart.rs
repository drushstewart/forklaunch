//! Carts.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_cart(state: &AppState) -> Handler {
    state.ecommerce.handler("createCart")
}

pub fn get_cart(state: &AppState) -> Handler {
    state.ecommerce.handler("getCart")
}

pub fn add_cart_item(state: &AppState) -> Handler {
    state.ecommerce.handler("addCartItem")
}

pub fn remove_cart_item(state: &AppState) -> Handler {
    state.ecommerce.handler("removeCartItem")
}

pub fn clear_cart(state: &AppState) -> Handler {
    state.ecommerce.handler("clearCart")
}
