//! Stripe and PayPal provider callbacks.
//! Handlers come from `forklaunch-ecommerce-stripe`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_ecommerce_stripe::http::Handler;

pub fn handle_stripe_webhook(state: &AppState) -> Handler {
    state.ecommerce.handler("handleStripeWebhook")
}

pub fn handle_paypal_webhook(state: &AppState) -> Handler {
    state.ecommerce.handler("handlePaypalWebhook")
}
