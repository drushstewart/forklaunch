//! Code-set description and procedure-code lookup.
//! Handlers come from `forklaunch-cac-base`; return your own
//! `axum::routing::any(handler)` to customize one.
use crate::registrations::AppState;
use forklaunch_cac_base::http::Handler;

pub fn describe_code_set(state: &AppState) -> Handler {
    state.cac.handler("describeCodeSet")
}

pub fn lookup_procedure_code(state: &AppState) -> Handler {
    state.cac.handler("lookupProcedureCode")
}
