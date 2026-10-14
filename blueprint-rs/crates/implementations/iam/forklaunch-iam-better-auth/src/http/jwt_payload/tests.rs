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
use std::sync::Mutex;
use tower::ServiceExt;
struct Signer(Arc<Mutex<Vec<Value>>>, Arc<Mutex<Vec<&'static str>>>);
impl JwtSigner for Signer {
    async fn sign(&self, payload: Value) -> BlueprintResult<String> {
        self.1.lock().unwrap().push("sign");
        self.0.lock().unwrap().push(payload);
        Ok("owned-token".into())
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn payload_replacement_precedence_failures_and_cookie_identity_match_actual_routes() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
        for mode in [
            "custom",
            "empty",
            "none",
            "nullClaims",
            "invalidIat",
            "invalidExp",
            "invalidIss",
            "invalidAud",
            "throw",
            "disabled",
        ] {
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
            let cookie = SessionCookie { secure: false }.serialize(token, &[42; 32], true);
            let cookie = cookie.split(';').next().unwrap();
            let order = Arc::new(Mutex::new(Vec::new()));
            let payload_order = order.clone();
            let subject_order = order.clone();
            let signed = Arc::new(Mutex::new(Vec::new()));
            let mut app=AuthHttp::new(store.clone(),"http://localhost:3000".into(),vec![42;32],vec![]).unwrap()
                .with_clock(move||now).with_jwt_header_disabled(mode=="disabled")
                .with_jwt_payload_resolver(move|context| {
                    assert_eq!(context.user()["id"],context.session()["userId"]);
                    assert!(!context.session()["token"].as_str().unwrap().is_empty());
                    assert!(!format!("{context:?}").contains("owner@example.test"));
                    payload_order.lock().unwrap().push("payload");
                    async move {
                        if mode=="throw" {return Err(BlueprintError::new("secret","private callback credential"));}
                        let claims=match mode {
                            "none"=>return Ok(None),
                            "custom"=>json!({"custom":true,"sub":"ignored","iat":123,"exp":456,"iss":"issuer","aud":["audience"]}),
                            "nullClaims"=>json!({"iat":null,"exp":null,"iss":null,"aud":null}),
                            "invalidIat"=>json!({"iat":{},"exp":456}),
                            "invalidExp"=>json!({"exp":{}}),
                            "invalidIss"=>json!({"iss":{}}),
                            "invalidAud"=>json!({"aud":[42]}),
                            _=>json!({}),
                        };Ok(Some(claims.as_object().unwrap().clone()))
                    }
                }).with_jwt_subject_resolver(move |_| {subject_order.lock().unwrap().push("subject");async {Ok(Some("resolved-subject".into()))}});
            if external {
                app = app.with_external_jwt_signer(
                    ExternalJwtSigner::new(
                        "https://keys.example.test/public",
                        "EdDSA",
                        Signer(signed.clone(), order.clone()),
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
                assert!(order.lock().unwrap().is_empty());
            }
            for path in ["/get-session", "/token"] {
                order.lock().unwrap().clear();
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
                let failed = mode == "throw" || (!external && mode.starts_with("invalid"));
                assert_eq!(
                    response.status().as_u16(),
                    if failed { 500 } else { 200 },
                    "{external}/{mode}/{path}"
                );
                let header = response
                    .headers()
                    .get("set-auth-jwt")
                    .map(|value| value.to_str().unwrap().to_owned());
                if failed {
                    assert!(response.headers().get("set-cookie").is_none());
                }
                let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
                if failed {
                    assert!(bytes.is_empty());
                    assert_eq!(
                        *order.lock().unwrap(),
                        if mode == "throw" {
                            vec!["payload"]
                        } else {
                            vec!["payload", "subject"]
                        }
                    );
                    {
                        let mut tx = store.begin(false).await.unwrap();
                        assert_eq!(
                            tx.list(Model::Jwks).await.unwrap().is_empty(),
                            mode == "throw"
                        );
                        tx.commit().await.unwrap();
                    }
                } else if mode == "disabled" && path == "/get-session" {
                    assert!(header.is_none());
                    assert!(order.lock().unwrap().is_empty());
                } else {
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    if path == "/get-session" {
                        assert_eq!(body["user"]["id"], "owner");
                    }
                    let encoded = if path == "/token" {
                        body["token"].as_str().unwrap().to_owned()
                    } else {
                        header.unwrap()
                    };
                    let claims = if external {
                        signed.lock().unwrap().last().unwrap().clone()
                    } else {
                        serde_json::from_slice::<Value>(
                            &URL_SAFE_NO_PAD
                                .decode(encoded.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        *order.lock().unwrap(),
                        if external {
                            vec!["payload", "subject", "sign"]
                        } else {
                            vec!["payload", "subject"]
                        }
                    );
                    assert_eq!(claims["sub"], "resolved-subject");
                    assert!(claims.get("email").is_none());
                    assert!(claims.get("sessionId").is_none());
                    if external {
                        match mode {
                            "invalidIat" => assert_eq!(claims["iat"], json!({})),
                            "invalidExp" => assert_eq!(claims["exp"], json!({})),
                            "invalidIss" => assert_eq!(claims["iss"], json!({})),
                            "invalidAud" => assert_eq!(claims["aud"], json!([42])),
                            _ => {}
                        }
                    }
                    if mode == "custom" {
                        assert_eq!(claims["iat"], 123);
                        assert_eq!(claims["exp"], 456);
                        assert_eq!(claims["iss"], "issuer");
                        assert_eq!(claims["aud"], json!(["audience"]));
                    }
                    if mode == "nullClaims" {
                        assert!(claims["iat"].is_null());
                        assert_eq!(claims["iss"], "http://localhost:3000");
                        assert_eq!(claims["exp"].as_f64(), Some(1_800_000_900.0));
                    }
                }
                let view = SessionService::new(store.clone())
                    .get(token, false, true, now)
                    .await;
                assert_eq!(view.unwrap().unwrap().user["id"], "owner");
            }
        }
    }
}
