//! Catalog imports by external ID.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn import_catalog(state: &AppState) -> Handler {
    state.ecommerce.handler("importCatalog")
}
