//! Shared endpoint paths and HTTP policy.
use crate::schemas::ErrorBody;
use framework_core::contract::{BodyContract, FieldContract, HttpMethod, RouteContract, SchemaRef};

pub const ATTACHMENTS: &str = "/v1/workspaces/{workspace_id}/notes/{note_id}/attachments";
pub const ATTACHMENT_CONTENT: &str =
    "/v1/workspaces/{workspace_id}/notes/{note_id}/attachments/{attachment_id}/content";

pub const NOTE_TEXT: &str = "/v1/workspaces/{workspace_id}/notes/{note_id}/export.txt";
pub const NOTE_HTML: &str = "/v1/workspaces/{workspace_id}/notes/{note_id}/export.html";
pub const NOTE_FORM: &str = "/v1/workspaces/{workspace_id}/notes/import";
pub const NOTES: &str = "/v1/workspaces/{workspace_id}/notes";
pub const NOTE: &str = "/v1/workspaces/{workspace_id}/notes/{note_id}";
pub const EVENTS: &str = "/v1/workspaces/{workspace_id}/events";
pub const PRESENCE: &str = "/v1/workspaces/{workspace_id}/presence";
pub const INSIGHTS: &str = "/v1/workspaces/{workspace_id}/notes/{note_id}/insights";
pub const INTERNAL_INSIGHT: &str = "/internal/workspaces/{workspace_id}/notes/{note_id}/insight";

pub(crate) fn route(name: &str, method: HttpMethod, path: &str) -> RouteContract {
    let mut route = RouteContract::new(name, name, method, path);
    if path.starts_with("/internal/") {
        let options = framework_core::contract::RouteOptions {
            mcp: false,
            openapi: false,
            ..Default::default()
        };
        route = route.with_options(options);
    } else {
        route = route.with_auth(framework_core::contract::AuthConfig::cookie(
            "better-auth.session_token",
        ));
        if path == EVENTS {
            let options = framework_core::contract::RouteOptions {
                mcp: false,
                ..Default::default()
            };
            route = route.with_options(options);
        }
    }

    for name in ["workspace_id", "note_id", "attachment_id"] {
        if path.contains(&format!("{{{name}}}")) {
            let mut field = FieldContract::required(name, "Identifier");
            field.schema = SchemaRef::uuid("Identifier");
            route = route.with_param(field);
        }
    }
    for status in [400, 401, 403, 404, 409, 413, 500, 503] {
        route = route.with_response(status, Some(BodyContract::json_type::<ErrorBody>()));
    }
    route
}
