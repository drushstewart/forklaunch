#![forbid(unsafe_code)]

//! A runnable sample service that wires the IAM blueprint (contracts +
//! in-memory implementation) into the framework's `ForkLaunchRouter`.
//!
//! This is the end-to-end proof that the stack composes: blueprint-defined
//! [`forklaunch_iam_interface::contracts`] drive a real `axum` server, with the
//! framework's contract-validation middleware in front of handlers backed by
//! [`InMemoryIamService`].

use std::sync::Arc;

use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_blueprint_core::{BlueprintModule, PageRequest, contract::RouteContract};
use forklaunch_core::ServiceInfo;
use forklaunch_iam_base::InMemoryIamService;
use forklaunch_iam_interface::{IamService, contracts};

/// Describes this blueprint module.
#[must_use]
pub fn module() -> BlueprintModule {
    BlueprintModule::new("sample-service", "Sample Rust blueprint service")
        .with_tag("kind", "service")
        .with_tag("language", "rust")
}

/// Returns a single IAM route contract by operation name.
fn iam_contract(operation_name: &str) -> RouteContract {
    contracts::registry()
        .routes()
        .iter()
        .find(|route| route.operation_name == operation_name)
        .cloned()
        .unwrap_or_else(|| panic!("iam contract `{operation_name}` should exist"))
}

/// Builds an IAM HTTP router backed by the given in-memory service.
///
/// Routes are registered from the blueprint's published contracts, so request
/// validation and the generated OpenAPI document stay in lockstep with the
/// service definition.
#[must_use]
pub fn iam_router(service: Arc<InMemoryIamService>) -> ForkLaunchRouter {
    let get_user_service = Arc::clone(&service);
    let list_users_service = Arc::clone(&service);

    ForkLaunchRouter::new(ServiceInfo::new("iam", "0.1.0"))
        .get(iam_contract("getUser"), move |Path(id): Path<String>| {
            let service = Arc::clone(&get_user_service);
            async move {
                match service.get_user(&id) {
                    Ok(Some(user)) => (StatusCode::OK, Json(user)).into_response(),
                    Ok(None) => StatusCode::NOT_FOUND.into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        })
        .get(iam_contract("listUsers"), move || {
            let service = Arc::clone(&list_users_service);
            async move {
                match service.list_users(PageRequest::first_page(100)) {
                    Ok(page) => (StatusCode::OK, Json(page.items)).into_response(),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use forklaunch_iam_interface::User;
    use tower::ServiceExt;

    use super::*;

    fn service() -> Arc<InMemoryIamService> {
        Arc::new(InMemoryIamService::new(vec![
            User::new("user_1", "ada@example.com"),
            User::new("user_2", "grace@example.com"),
        ]))
    }

    #[test]
    fn sample_service_describes_module() {
        assert_eq!(module().name, "sample-service");
    }

    #[tokio::test]
    async fn iam_router_serves_a_user_over_http() {
        let app = iam_router(service()).into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/user_1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("user_1"));
        assert!(body.contains("ada@example.com"));
    }

    #[tokio::test]
    async fn iam_router_returns_404_for_unknown_user() {
        let app = iam_router(service()).into_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/users/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn iam_router_lists_users_and_serves_openapi() {
        let app = iam_router(service()).into_router();

        let listed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(listed.status(), StatusCode::OK);
        let bytes = to_bytes(listed.into_body(), 1 << 20).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("user_1") && body.contains("user_2"));

        // the framework serves an OpenAPI document derived from the same contracts
        let openapi = app
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(openapi.status(), StatusCode::OK);
        let bytes = to_bytes(openapi.into_body(), 1 << 20).await.unwrap();
        let document = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(document.contains("\"openapi\":\"3.1.0\""));
        assert!(document.contains("getUser"));
        assert!(document.contains("listUsers"));
    }
}
