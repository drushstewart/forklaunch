use super::*;
use crate::{
    cookies::SessionCookie,
    http::ExternalJwtSigner,
    keyring::custom::JwtSigner,
    sessions::SessionService,
    store::{AuthTransaction, Model, memory::MemoryAuthStore},
};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::sync::Mutex;
use tower::ServiceExt;

struct CapturingSigner(Arc<Mutex<Vec<Value>>>);
impl JwtSigner for CapturingSigner {
    async fn sign(&self, payload: Value) -> BlueprintResult<String> {
        self.0.lock().unwrap().push(payload);
        Ok("owned-signed-token".into())
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn subject_policy_preserves_session_identity_and_precedes_both_signers() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
        for mode in ["custom", "empty", "fallback", "failure", "disabled"] {
            let store = Arc::new(MemoryAuthStore::default());
            let mut tx = store.begin(true).await.unwrap();
            tx.insert(
                Model::User,
                json!({"id":"owner","email":"owner@example.test"}),
            )
            .await
            .unwrap();
            tx.commit().await.unwrap();
            let session = SessionService::new(store.clone())
                .create("owner", "", "", now)
                .await
                .unwrap();
            let token = session["token"].as_str().unwrap();
            let encoded = SessionCookie { secure: false }.serialize(token, &[42; 32], true);
            let cookie = encoded.split(';').next().unwrap();
            let contexts = Arc::new(Mutex::new(Vec::new()));
            let capture = contexts.clone();
            let signed = Arc::new(Mutex::new(Vec::new()));
            let events = Arc::new(Mutex::new(Vec::new()));
            let capture_event = events.clone();
            let mut app = AuthHttp::new(
                store.clone(),
                "http://localhost:3000".into(),
                vec![42; 32],
                vec![],
            )
            .unwrap()
            .with_clock(move || now)
            .with_api_error_observer_mode(crate::http::AuthApiErrorObserverMode::UnexpectedOnly)
            .with_api_error_observer(move |event| capture_event.lock().unwrap().push(event.clone()))
            .with_jwt_header_disabled(mode == "disabled")
            .with_jwt_subject_resolver(move |context| {
                capture.lock().unwrap().push(context);
                async move {
                    match mode {
                        "failure" => Err(BlueprintError::new(
                            "private_callback_code",
                            "private callback credential",
                        )),
                        "fallback" => Ok(None),
                        "empty" => Ok(Some(String::new())),
                        _ => Ok(Some("custom:owner".into())),
                    }
                }
            });
            if external {
                app = app.with_external_jwt_signer(
                    ExternalJwtSigner::new(
                        "https://keys.example.test/public",
                        "EdDSA",
                        CapturingSigner(signed.clone()),
                    )
                    .unwrap(),
                );
            }
            let router = app.router();
            for (path, status) in [("/token", 401), ("/get-session", 200)] {
                let response = router
                    .clone()
                    .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status().as_u16(), status);
                assert!(contexts.lock().unwrap().is_empty());
            }
            for path in ["/get-session", "/token"] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .header("cookie", cookie)
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    if mode == "failure" { 500 } else { 200 }
                );
                let header = response
                    .headers()
                    .get("set-auth-jwt")
                    .map(|value| value.to_str().unwrap().to_owned());
                if mode == "failure" {
                    assert!(response.headers().get("set-cookie").is_none());
                }
                let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
                if mode == "failure" {
                    assert!(bytes.is_empty());
                    assert!(signed.lock().unwrap().is_empty());
                    let mut tx = store.begin(false).await.unwrap();
                    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
                    tx.commit().await.unwrap();
                } else if mode == "disabled" && path == "/get-session" {
                    assert!(header.is_none());
                    assert!(contexts.lock().unwrap().is_empty());
                } else {
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    let encoded = if path == "/token" {
                        body["token"].as_str().unwrap().to_owned()
                    } else {
                        header.unwrap()
                    };
                    let payload: Value = if external {
                        signed.lock().unwrap().last().unwrap().clone()
                    } else {
                        serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(encoded.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        payload["sub"],
                        match mode {
                            "empty" => "",
                            "fallback" => "owner",
                            _ => "custom:owner",
                        }
                    );
                    assert_eq!(payload["sessionId"], session["id"]);
                    assert_eq!(payload["email"], "owner@example.test");
                    if path == "/get-session" {
                        assert_eq!(body["user"]["id"], "owner");
                    }
                }
            }
            {
                let contexts = contexts.lock().unwrap();
                assert_eq!(contexts.len(), if mode == "disabled" { 1 } else { 2 });
                for context in contexts.iter() {
                    assert_eq!(context.user()["id"], "owner");
                    assert_eq!(context.session()["id"], session["id"]);
                    assert_eq!(context.session()["userId"], "owner");
                    assert_eq!(format!("{context:?}"), "JwtSessionContext { .. }");
                }
            }
            assert_eq!(
                events.lock().unwrap().len(),
                if mode == "failure" { 2 } else { 0 }
            );
            let mut tx = store.begin(false).await.unwrap();
            assert_eq!(
                tx.get(Model::Session, session["id"].as_str().unwrap())
                    .await
                    .unwrap()
                    .unwrap()["userId"],
                "owner"
            );
            if external {
                assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
            }
            tx.commit().await.unwrap();
        }
    }
}
