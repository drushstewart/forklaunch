//! Billing portal sessions.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn create_billing_portal_session(state: &AppState) -> Handler {
    state.billing.handler("createBillingPortalSession")
}

pub fn get_billing_portal_session(state: &AppState) -> Handler {
    state.billing.handler("getBillingPortalSession")
}

pub fn update_billing_portal_session(state: &AppState) -> Handler {
    state.billing.handler("updateBillingPortalSession")
}

pub fn expire_billing_portal_session(state: &AppState) -> Handler {
    state.billing.handler("expireBillingPortalSession")
}
