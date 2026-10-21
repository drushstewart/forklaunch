//! Product variants.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn create_variant(state: &AppState) -> Handler {
    state.ecommerce.handler("createVariant")
}

pub fn list_variants(state: &AppState) -> Handler {
    state.ecommerce.handler("listVariants")
}

pub fn list_variants_by_product(state: &AppState) -> Handler {
    state.ecommerce.handler("listVariantsByProduct")
}

pub fn get_variant(state: &AppState) -> Handler {
    state.ecommerce.handler("getVariant")
}

pub fn update_variant(state: &AppState) -> Handler {
    state.ecommerce.handler("updateVariant")
}

pub fn delete_variant(state: &AppState) -> Handler {
    state.ecommerce.handler("deleteVariant")
}
