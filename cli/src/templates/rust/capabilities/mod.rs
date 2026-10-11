// BEGIN FORKLAUNCH MANAGED CAPABILITIES
__EVENT_MODULES__

use forklaunch_axum::ForkLaunchRouter;
use forklaunch_blueprint_core::{BlueprintResult, instance_gateway::InstanceGatewayClient,
    platform_events::{MemoryPlatformEventNonces, PlatformEventVerifier}};
use forklaunch_contract::{BodyContract, ContractRegistry, FieldContract, RouteContract, BodyKind, SchemaRef};
use axum::{RequestExt, body::to_bytes, extract::{Path, Request}, http::StatusCode, middleware::{self, Next}, response::IntoResponse};
use std::sync::{Arc, OnceLock};

fn instance_gateway() -> BlueprintResult<InstanceGatewayClient> {
    // Reuse the connection pool after the first successful resolution. A
    // configuration error is not cached, so it can be corrected by the caller.
    static GATEWAY: OnceLock<InstanceGatewayClient> = OnceLock::new();
    if let Some(gateway) = GATEWAY.get() { return Ok(gateway.clone()); }
    let gateway = InstanceGatewayClient::from_env()?;
    Ok(GATEWAY.get_or_init(|| gateway).clone())
}

__CLIENT_FACTORIES__

fn text_body() -> BodyContract { BodyContract {kind:BodyKind::Text,schema:SchemaRef::named("String"),content_type:Some("text/plain".into())} }

pub fn contracts() -> Vec<RouteContract> {
    vec![RouteContract::post("ReceivePlatformEvent", "Receives a signed event from the ForkLaunch platform", "/platform-events/:feature")
        .with_param(FieldContract::required("feature", "String"))
        .with_request_body(BodyContract::json("PlatformEvent"))
        .with_response(200, Some(text_body()))
        .with_response(401, Some(text_body()))
        .with_response(404, Some(text_body()))
        .with_response(500, Some(text_body()))
        .with_response(503, Some(text_body()))]
}

pub fn register_contracts(registry: &mut ContractRegistry) {
    if !registry.routes().iter().any(|route| route.path == "/platform-events/:feature") {
        for contract in contracts() { registry.register(contract); }
    }
}

pub trait ManagedCapabilitiesRouterExt {
    fn with_managed_capabilities(self) -> Self;
}
impl ManagedCapabilitiesRouterExt for ForkLaunchRouter {
    fn with_managed_capabilities(self) -> Self {
        let original = self.registry().routes().iter().any(|route| route.path == "/platform-events/:feature");
        let router = if original { self } else {
            self.post(contracts().remove(0), |Path(feature): Path<String>| async move {
                (StatusCode::NOT_FOUND, format!("No handler for {feature} events"))
            })
        };
        // One verifier covers all features. Applications running replicas can
        // replace the memory store with a shared PlatformEventNonceStore.
        let verifier = Arc::new(std::env::var("INSTANCE_HMAC_KEY").ok().and_then(|key|
            PlatformEventVerifier::new(key, Arc::new(MemoryPlatformEventNonces::default())).ok()));
        router.map_route("/platform-events/:feature", move |route| {
            let verifier = Arc::clone(&verifier);
            route.layer(middleware::from_fn(move |request: Request, next: Next| {
                let verifier = Arc::clone(&verifier);
                async move {
                    let mut request = request;
                    let path = request.uri().path().to_owned();
                    let feature = match request.extract_parts::<Path<String>>().await {
                        Ok(Path(feature)) => feature,
                        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid event feature").into_response(),
                    };
                    let known = matches!(feature.as_str(), __KNOWN_FEATURES__);
                    let forward_payment = original && feature == "payments";
                    if request.method() != axum::http::Method::POST || (!known && !forward_payment) {
                        return next.run(request).await;
                    }
                    let Some(verifier) = verifier.as_ref() else {
                        return (StatusCode::UNAUTHORIZED, "Event verification failed").into_response();
                    };
                    let (parts, body) = request.into_parts();
                    let authorization = parts.headers.get("authorization").and_then(|value|value.to_str().ok()).unwrap_or_default();
                    let Ok(body) = to_bytes(body, 1024*1024).await else {
                        return (StatusCode::UNAUTHORIZED, "Event verification failed").into_response();
                    };
                    let event = match verifier.verify("POST", &path, authorization, &body).await {
                        Ok(event) => event,
                        Err(error) => {
                            let status = if matches!(error.code.as_str(), "platform_event_nonce_capacity"|"platform_event_nonce_store") {
                                StatusCode::SERVICE_UNAVAILABLE
                            } else { StatusCode::UNAUTHORIZED };
                            return (status, "Event verification failed").into_response();
                        }
                    };
                    if forward_payment {
                        // Keep the existing billing receipt and business effects.
                        return next.run(Request::from_parts(parts, axum::body::Body::from(body))).await;
                    }
                    let result: BlueprintResult<()> = match feature.as_str() {
__HANDLER_ARMS__
                        _ => unreachable!("configured feature"),
                    };
                    match result {
                        Ok(()) => (StatusCode::OK, "ok").into_response(),
                        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Platform event handling failed").into_response(),
                    }
                }
            }))
        }).expect("platform event route was registered")
    }
}
// END FORKLAUNCH MANAGED CAPABILITIES
