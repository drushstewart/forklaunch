use super::*;
use crate::{jwt::SigningKey, keyring::custom::JwtSigner};
use std::sync::atomic::AtomicUsize;

struct Signer {
    key: SigningKey,
    calls: Arc<AtomicUsize>,
    fail: bool,
}
impl JwtSigner for Signer {
    async fn sign(&self, payload: Value) -> BlueprintResult<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(BlueprintError::new(
                "private",
                "private provider credentials",
            ))
        } else {
            self.key.sign_payload(&payload)
        }
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn live_external_signing_requires_session_and_never_publishes_or_persists_local_keys() {
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let secret = b"external-jwt-fixture-secret-32-characters";
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"external-user","email":"external@fixture.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(store.clone())
        .create("external-user", "", "", now)
        .await
        .unwrap();
    let cookie =
        SessionCookie { secure: false }.serialize(session["token"].as_str().unwrap(), secret, true);
    let cookie = cookie.split(';').next().unwrap();
    for fail in [false, true] {
        let key = SigningKey::generate("external-key").unwrap();
        let jwks = json!({"keys":[key.public_jwk()]});
        let calls = Arc::new(AtomicUsize::new(0));
        let signer = ExternalJwtSigner::new(
            "https://keys.fixture.test/jwks",
            "EdDSA",
            Signer {
                key,
                calls: calls.clone(),
                fail,
            },
        )
        .unwrap();
        assert_eq!(signer.remote_jwks_url(), "https://keys.fixture.test/jwks");
        assert_eq!(signer.algorithm(), "EdDSA");
        let app = AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            secret.to_vec(),
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_external_jwt_signer(signer);
        #[cfg(feature = "application-http")]
        let app = app.router_with_redirects();
        #[cfg(not(feature = "application-http"))]
        let app = app.router();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let base = format!("http://{address}");
        assert_eq!(
            client
                .get(format!("{base}/jwks"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            client
                .get(format!("{base}/token"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for path in ["/token", "/get-session"] {
            let response = client
                .get(format!("{base}{path}"))
                .header("cookie", cookie)
                .send()
                .await
                .unwrap();
            if fail {
                assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
                assert!(!response.text().await.unwrap().contains("private"));
            } else {
                assert_eq!(response.status(), StatusCode::OK);
                let token = if path == "/token" {
                    response.json::<Value>().await.unwrap()["token"]
                        .as_str()
                        .unwrap()
                        .to_owned()
                } else {
                    response.headers()["set-auth-jwt"]
                        .to_str()
                        .unwrap()
                        .to_owned()
                };
                let claims = verify_jwt(&token, &jwks);
                assert_eq!(
                    claims,
                    json!({"iat":now.timestamp(),"exp":now.timestamp()+900,
                    "sub":"external-user","sessionId":session["id"],"email":"external@fixture.test","activeOrganizationId":null,
                    "iss":"http://localhost:3000","aud":"http://localhost:3000"})
                );
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        #[cfg(feature = "application-http")]
        {
            let response = client
                .get(format!(
                    "{base}/test-callback?callbackUrl=http%3A%2F%2Flocalhost%3A3000%2Fdone"
                ))
                .header("cookie", cookie)
                .header("accept", "application/json")
                .send()
                .await
                .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 4);
            assert_eq!(response.status(), StatusCode::FOUND);
            let destination =
                url::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
            let fields: std::collections::HashMap<_, _> =
                url::form_urlencoded::parse(destination.fragment().unwrap().as_bytes())
                    .into_owned()
                    .collect();
            if fail {
                assert_eq!(fields["token"], "");
            } else {
                assert_eq!(verify_jwt(&fields["token"], &jwks)["sub"], "external-user");
            }
        }
        let authenticated_calls = calls.load(Ordering::SeqCst);
        SessionService::new(store.clone())
            .sign_out(session["token"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            client
                .get(format!("{base}/token"))
                .header("cookie", cookie)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(calls.load(Ordering::SeqCst), authenticated_calls);
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.list_page(Model::Jwks, None, 100)
                .await
                .unwrap()
                .records
                .is_empty()
        );
        tx.commit().await.unwrap();
        stop.send(()).unwrap();
        server.await.unwrap();
        // Restore the same owned session for the failure branch.
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::Session, session.clone()).await.unwrap();
        tx.commit().await.unwrap();
    }
}

#[test]
fn external_signer_configuration_requires_explicit_public_key_metadata() {
    for (url, algorithm) in [
        ("", "EdDSA"),
        ("/jwks", "EdDSA"),
        ("file:///tmp/jwks", "EdDSA"),
        ("https://user:secret@keys.test/jwks", "EdDSA"),
        ("https://keys.test/jwks#private", "EdDSA"),
        ("https://keys.test/jwks", ""),
        ("https://keys.test/jwks", "HS256"),
    ] {
        let signer = Signer {
            key: SigningKey::generate("external-key").unwrap(),
            calls: Arc::new(AtomicUsize::new(0)),
            fail: false,
        };
        assert!(ExternalJwtSigner::new(url, algorithm, signer).is_err());
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn disabled_session_jwt_header_skips_signing_but_keeps_explicit_token_and_jwks() {
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
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
        let secret = vec![42; 32];
        let encoded = SessionCookie { secure: false }.serialize(
            session["token"].as_str().unwrap(),
            &secret,
            true,
        );
        let cookie = encoded.split(';').next().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut app = AuthHttp::new(
            store.clone(),
            "http://localhost:3000".into(),
            secret,
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
        .with_jwt_header_disabled(true);
        if external {
            app = app.with_external_jwt_signer(
                ExternalJwtSigner::new(
                    "https://keys.example.test/jwks",
                    "EdDSA",
                    Signer {
                        key: SigningKey::generate("external").unwrap(),
                        calls: calls.clone(),
                        fail: true,
                    },
                )
                .unwrap(),
            );
        }
        let router = app.router();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/get-session")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("set-auth-jwt").is_none());
        assert!(
            response
                .headers()
                .get("access-control-expose-headers")
                .is_none()
        );
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
        assert_eq!(body["session"]["id"], session["id"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let mut tx = store.begin(false).await.unwrap();
        assert!(
            tx.list(Model::Jwks).await.unwrap().is_empty(),
            "session read must not mint keys"
        );
        tx.commit().await.unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/token")
                    .header("cookie", cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if external {
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "explicit issuance still invokes signer"
            );
        } else {
            assert_eq!(response.status(), StatusCode::OK);
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert!(body["token"].is_string());
        }
        let response = router
            .oneshot(Request::builder().uri("/jwks").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if external {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::OK
            }
        );
        if !external {
            let body: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(body["keys"].as_array().unwrap().len(), 1);
        }
    }
}
