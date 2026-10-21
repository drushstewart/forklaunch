//! Plans.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn create_plan(state: &AppState) -> Handler {
    state.billing.handler("createPlan")
}

pub fn list_plans(state: &AppState) -> Handler {
    state.billing.handler("listPlans")
}

pub fn get_plan(state: &AppState) -> Handler {
    state.billing.handler("getPlan")
}

pub fn update_plan(state: &AppState) -> Handler {
    state.billing.handler("updatePlan")
}

pub fn delete_plan(state: &AppState) -> Handler {
    state.billing.handler("deletePlan")
}
