//! Products and the public catalog.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_product(state: &AppState) -> Handler {
    state.ecommerce.handler("createProduct")
}

pub fn list_products(state: &AppState) -> Handler {
    state.ecommerce.handler("listProducts")
}

pub fn list_catalog(state: &AppState) -> Handler {
    state.ecommerce.handler("listCatalog")
}

pub fn get_product_by_handle(state: &AppState) -> Handler {
    state.ecommerce.handler("getProductByHandle")
}

pub fn get_product(state: &AppState) -> Handler {
    state.ecommerce.handler("getProduct")
}

pub fn update_product(state: &AppState) -> Handler {
    state.ecommerce.handler("updateProduct")
}

pub fn delete_product(state: &AppState) -> Handler {
    state.ecommerce.handler("deleteProduct")
}
