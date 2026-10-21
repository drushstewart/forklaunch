use crate::api::controllers::exports::html_escape;
use crate::registrations::AppState;
use crate::server::router;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::get,
};
use fieldnotes_contracts::{INTERNAL_INSIGHT, NOTES, PatchNote, registry};
use fieldnotes_security::SecurityService;
use tower::ServiceExt;

fn application() -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@localhost/unused")
        .unwrap();
    let security = SecurityService::new("http://127.0.0.1:9".into(), "x".repeat(32)).unwrap();
    router(
        AppState::new(
            security,
            crate::NotesRepository::new(pool, fieldnotes_security::EnvelopeKey::new([1; 32]))
                .unwrap(),
            "http://localhost:8080".into(),
        )
        .expect("valid domain mapping schemas"),
    )
    .expect("valid test origin and contracts")
}
#[tokio::test]
async fn native_escape_hatch_docs_and_auth_fail_closed() {
    let app = application().route("/native", get(|| async { "native" }));
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri("/native")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let document: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(document["paths"].get(NOTES).is_some());
    assert!(document["paths"].get(INTERNAL_INSIGHT).is_none());
    assert!(document["paths"].get("/native").is_none());
    let uri = "/v1/workspaces/00000000-0000-0000-0000-000000000001/notes";
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(r#"{"title":"hello","body":"world"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("origin", "https://attacker.invalid")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
#[test]
fn tool_registry_excludes_internal_and_streaming_routes() {
    let tools = framework_core::mcp::mcp_tools(&registry());
    let names = format!("{tools:?}");
    assert!(!names.contains("applyInsight"));
    assert!(!names.contains("watchNotes"));
    assert!(names.contains("createNote"));
}
#[test]
fn html_export_escapes_all_markup_and_preserves_unicode() {
    assert_eq!(
        html_escape("<script>\"'&📝"),
        "&lt;script&gt;&quot;&#39;&amp;📝"
    );
}
#[test]
fn patch_requires_revision_and_rejects_unknown_fields() {
    assert!(serde_json::from_str::<PatchNote>(r#"{"title":"new"}"#).is_err());
    assert!(
        serde_json::from_str::<PatchNote>(
            r#"{"expectedRevision":1,"title":"new","workspaceId":"forged"}"#
        )
        .is_err()
    );
    let patch: PatchNote = serde_json::from_str(r#"{"expectedRevision":1,"title":"new"}"#).unwrap();
    assert_eq!(patch.expected_revision, 1);
    assert!(serde_json::from_str::<PatchNote>(r#"{"expectedRevision":1,"title":null}"#).is_err());
    let omitted: PatchNote = serde_json::from_str(r#"{"expectedRevision":1}"#).unwrap();
    assert!(omitted.title.is_missing());
    assert_eq!(
        serde_json::to_value(omitted).unwrap(),
        serde_json::json!({"expectedRevision":1})
    );
}
