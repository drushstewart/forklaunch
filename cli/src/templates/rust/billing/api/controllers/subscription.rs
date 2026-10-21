//! Subscriptions.
//! Handlers come from `forklaunch-billing-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_billing_stripe::http::Handler;

pub fn create_subscription(state: &AppState) -> Handler {
    state.billing.handler("createSubscription")
}

pub fn list_subscriptions(state: &AppState) -> Handler {
    state.billing.handler("listSubscriptions")
}

pub fn get_subscription(state: &AppState) -> Handler {
    state.billing.handler("getSubscription")
}

pub fn get_user_subscription(state: &AppState) -> Handler {
    state.billing.handler("getUserSubscription")
}

pub fn get_organization_subscription(state: &AppState) -> Handler {
    state.billing.handler("getOrganizationSubscription")
}

pub fn update_subscription(state: &AppState) -> Handler {
    state.billing.handler("updateSubscription")
}

pub fn delete_subscription(state: &AppState) -> Handler {
    state.billing.handler("deleteSubscription")
}

pub fn cancel_subscription(state: &AppState) -> Handler {
    state.billing.handler("cancelSubscription")
}

pub fn resume_subscription(state: &AppState) -> Handler {
    state.billing.handler("resumeSubscription")
}
