use crate::{
    AppState,
    domain::{Record, RecordSummary, records_json},
    error::Result,
};
use forklaunch_contract::{BodyContract, BodyKind, RouteContract, SchemaRef};
use forklaunch_http::{ErrorKind, ErrorResponse, FrameworkResponse, LocalRouter};

pub const ROUTE_NAME: &str = "{{{router_name}}}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct {{response_name}} {
    pub route: &'static str,
    pub summary: RecordSummary,
    pub records: Vec<Record>,
}

pub fn list(state: &AppState) -> Result<{{response_name}}> {
    let records = state.repository().list()?;
    let summary = RecordSummary::new(state.config().name.clone(), records.len());
    Ok({{response_name}} {
        route: ROUTE_NAME,
        summary,
        records,
    })
}

#[must_use]
pub fn path() -> String {
    format!("/{ROUTE_NAME}")
}

impl {{response_name}} {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{}\"route\":{},\"summary\":{},\"records\":{}{}",
            "{",
            forklaunch_common::json_string(self.route),
            self.summary.to_json(),
            records_json(&self.records),
            "}"
        )
    }
}

#[must_use]
pub fn contract() -> RouteContract {
    RouteContract::get(
        "List {{{route_title}}}",
        format!("List {} records", ROUTE_NAME),
        path(),
    )
    .with_response(200, Some(response_body()))
}

pub fn register(router: &mut LocalRouter, state: AppState) {
    router.register(contract(), move |_request| match list(&state) {
        Ok(response) => Ok(FrameworkResponse::json(200, response.to_json())),
        Err(error) => Ok(ErrorResponse::new(ErrorKind::Internal, error.to_string()).to_response()),
    });
}

fn response_body() -> BodyContract {
    BodyContract {
        kind: BodyKind::Json,
        schema: SchemaRef::named("{{{response_name}}}").with_json_schema(
            r#"{"type":"object","required":["route","summary","records"],"properties":{"route":{"type":"string","const":"{{{router_name}}}"},"summary":{"type":"object","required":["service","total"],"properties":{"service":{"type":"string"},"total":{"type":"integer","minimum":0}}},"records":{"type":"array","items":{"type":"object","required":["id","name"],"properties":{"id":{"type":"string"},"name":{"type":"string"}}}}}}"#,
        ),
        content_type: Some("application/json".to_string()),
    }
}
