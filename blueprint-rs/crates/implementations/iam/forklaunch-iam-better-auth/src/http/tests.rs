use super::*;
mod organizations;
pub(crate) mod tokens;
use crate::store::{AuthTransaction, Model, memory::MemoryAuthStore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use forklaunch_axum::{ForkLaunchRouter, RouterConfig};
use forklaunch_contract::{BodyContract, RouteContract};
use forklaunch_core::ServiceInfo;
use std::sync::atomic::{AtomicI64, Ordering};

fn verify_jwt(token: &str, jwks: &Value) -> Value {
    let pieces: Vec<_> = token.split('.').collect();
    assert_eq!(pieces.len(), 3);
    let header: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(pieces[0]).unwrap()).unwrap();
    let key = jwks["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["kid"] == header["kid"])
        .unwrap();
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ED25519,
        URL_SAFE_NO_PAD.decode(key["x"].as_str().unwrap()).unwrap(),
    )
    .verify(
        format!("{}.{}", pieces[0], pieces[1]).as_bytes(),
        &URL_SAFE_NO_PAD.decode(pieces[2]).unwrap(),
    )
    .unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(pieces[1]).unwrap()).unwrap()
}

#[tokio::test]
async fn secure_http_requires_secure_cookie_name_and_clears_separate_secure_headers() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let secret = b"secure-auth-http-fixture-only-secret-32";
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"secure-user","email":"secure@fixture.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(store.clone())
        .create("secure-user", "", "", now)
        .await
        .unwrap();
    let token = session["token"].as_str().unwrap();
    let app = AuthHttp::new(
        store,
        "https://auth.fixture.test".into(),
        secret.to_vec(),
        vec![],
    )
    .unwrap()
    .with_clock(move || now)
    .router();
    let cookie = SessionCookie { secure: true };
    let valid = cookie
        .serialize(token, secret, true)
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let insecure = SessionCookie { secure: false }
        .serialize(token, secret, true)
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    for (header, authenticated) in [(&valid, true), (&insecure, false)] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/get-session")
                    .header("cookie", header)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(!body.is_null(), authenticated);
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/sign-out")
                .method("POST")
                .header("cookie", valid)
                .header("origin", "https://auth.fixture.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get_all("set-cookie").iter().count(), 4);
    for header in response.headers().get_all("set-cookie") {
        let header = header.to_str().unwrap();
        assert!(header.starts_with("__Secure-better-auth."));
        assert!(header.contains("; Secure;"));
        assert!(header.contains("Max-Age=0"));
    }
}

#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn live_auth_http_mcp_signed_sessions_jwks_refresh_csrf_and_shutdown() {
    exercise_http(Arc::new(MemoryAuthStore::default())).await;
}

#[allow(clippy::too_many_lines)]
pub(crate) async fn exercise_http<S: AuthStore + 'static>(store: Arc<S>) {
    let start = 1_800_000_000_i64;
    let now = DateTime::<Utc>::from_timestamp(start, 0).unwrap();
    let clock = Arc::new(AtomicI64::new(start));
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, json!({"id":"http-owner","email":"http-owner@fixture.test","name":"Owner","firstName":"First","lastName":"Last","privateSecret":"never-public"})).await.unwrap();
    tx.commit().await.unwrap();
    let session = SessionService::new(store.clone())
        .create("http-owner", "", "", now)
        .await
        .unwrap();
    let secret = b"auth-http-fixture-only-secret-32-characters";
    let cookie = SessionCookie { secure: false };
    let signed = cookie
        .serialize(session["token"].as_str().unwrap(), secret, true)
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let owned_clock = clock.clone();
    let auth = AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        secret.to_vec(),
        vec![],
    )
    .unwrap()
    .with_clock(move || {
        DateTime::<Utc>::from_timestamp(owned_clock.load(Ordering::SeqCst), 0).unwrap()
    });
    let cors = auth.cors_layer();
    let auth = auth.router();
    let router = ForkLaunchRouter::new(ServiceInfo::new("native-auth-test", "1.0"))
        .get(
            RouteContract::get("Health", "Health", "/health")
                .with_response(200, Some(BodyContract::json("Health"))),
            || async { Json(json!({"ok":true})) },
        )
        .nest_internal("/api/auth", auth)
        .with_config(RouterConfig {
            cors: Some(cors),
            ..RouterConfig::default()
        })
        .into_router_with_default_mcp()
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let base = format!("http://{address}");
    let client = reqwest::Client::new();
    exercise_http_credentials(&client, &base).await;
    let updated = client
        .post(format!("{base}/api/auth/update-user"))
        .header("cookie", &signed)
        .header("origin", "http://localhost:3000")
        .json(&json!({"firstName":"Updated", "privateSecret":"injected"}))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::OK);
    assert!(updated.headers().get("set-cookie").is_some());
    let accounts = client
        .get(format!("{base}/api/auth/list-accounts"))
        .header("cookie", &signed)
        .send()
        .await
        .unwrap();
    assert_eq!(accounts.status(), StatusCode::OK);
    assert_eq!(accounts.json::<Value>().await.unwrap(), json!([]));
    let mut tx = store.begin(false).await.unwrap();
    let stored = tx.get(Model::User, "http-owner").await.unwrap().unwrap();
    assert_eq!(stored["firstName"], "Updated");
    assert_eq!(stored["privateSecret"], "never-public");
    tx.commit().await.unwrap();

    for (path, body, status) in [
        (
            "request-password-reset",
            json!({"email":"disabled@fixture.test"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "send-verification-email",
            json!({"email":"disabled@fixture.test"}),
            StatusCode::BAD_REQUEST,
        ),
        (
            "change-email",
            json!({"newEmail":"changed@fixture.test"}),
            StatusCode::BAD_REQUEST,
        ),
        ("delete-user", json!({}), StatusCode::NOT_FOUND),
    ] {
        let response = client
            .post(format!("{base}/api/auth/{path}"))
            .header("cookie", &signed)
            .header("origin", "http://localhost:3000")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }

    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::Verification, json!({"id":"http-reset","identifier":"reset-password:http-token","value":"http-owner","createdAt":now.to_rfc3339(),"expiresAt":(now + chrono::Duration::minutes(1)).to_rfc3339()})).await.unwrap();
    tx.commit().await.unwrap();

    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let redirect = browser
        .get(format!(
            "{base}/api/auth/reset-password/http-token?callbackURL=%2Fdashboard"
        ))
        .header("cookie", "")
        .header("origin", "https://evil.test")
        .send()
        .await
        .unwrap();
    assert_eq!(redirect.status(), StatusCode::FOUND);
    assert_eq!(
        redirect.headers()["location"],
        "http://localhost:3000/dashboard?token=http-token"
    );
    assert_eq!(redirect.headers()["content-type"], "application/json");
    assert!(redirect.text().await.unwrap().is_empty());
    let disabled = browser
        .get(format!(
            "{base}/api/auth/delete-user/callback?token=unused&callbackURL=%2Fdashboard"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(disabled.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        disabled.json::<Value>().await.unwrap(),
        json!({"message":"Not found","code":"NOT_FOUND"})
    );
    let reset = client
        .post(format!("{base}/api/auth/reset-password"))
        .json(&json!({"token":"http-token", "newPassword":"http-reset-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(reset.status(), StatusCode::OK);
    let verified = client
        .post(format!("{base}/api/auth/verify-password"))
        .header("cookie", &signed)
        .header("origin", "http://localhost:3000")
        .json(&json!({"password":"http-reset-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(verified.status(), StatusCode::OK);
    exercise_http_email(&client, &base, &store, secret, now).await;
    organizations::exercise(&client, &base, &signed, &store).await;
    let docs: Value = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(docs["paths"].get("/health").is_some());
    assert!(
        docs["paths"]
            .as_object()
            .unwrap()
            .keys()
            .all(|path| !path.starts_with("/api/auth"))
    );
    let tools: Value = client
        .post(format!("{base}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "{tools:?}");
    let invoked: Value = client.post(format!("{base}/mcp")).header("accept","application/json, text/event-stream")
        .header("mcp-protocol-version","2025-11-25").json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tools[0]["name"],"arguments":{}}})).send().await.unwrap().json().await.unwrap();
    assert_ne!(invoked["result"]["isError"], true, "{invoked}");
    assert!(invoked.get("error").is_none(), "{invoked}");
    let get = || {
        client
            .get(format!("{base}/api/auth/get-session"))
            .header("cookie", &signed)
    };
    let response = get()
        .header("origin", "http://localhost:3000")
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.headers()["access-control-allow-origin"],
        "http://localhost:3000"
    );
    assert_eq!(
        response.headers()["access-control-allow-credentials"],
        "true"
    );
    let cross = get()
        .header("origin", "https://evil.test")
        .send()
        .await
        .unwrap();
    assert!(cross.headers().get("access-control-allow-origin").is_none());
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(
        response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .contains("set-auth-jwt")
    );
    let jwt = response.headers()["set-auth-jwt"]
        .to_str()
        .unwrap()
        .to_owned();
    let view: Value = response.json().await.unwrap();
    assert_eq!(view["user"]["id"], "http-owner");
    assert!(view["user"].get("privateSecret").is_none());
    let jwks: Value = client
        .get(format!("{base}/api/auth/jwks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let claims = verify_jwt(&jwt, &jwks);
    assert_eq!(claims["sub"], "http-owner");
    assert_eq!(claims["iss"], "http://localhost:3000");
    assert_eq!(claims["aud"], "http://localhost:3000");
    assert_eq!(claims["exp"], start + 900);
    let issued: Value = client
        .get(format!("{base}/api/auth/token"))
        .header("cookie", &signed)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(verify_jwt(issued["token"].as_str().unwrap(), &jwks), claims);
    let unauth = client
        .get(format!("{base}/api/auth/token"))
        .header("cookie", "better-auth.session_token=forged")
        .send()
        .await
        .unwrap();
    assert_eq!(unauth.status(), 401);
    let forged: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", "better-auth.session_token=forged")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(forged, Value::Null);
    clock.store(start + 3600, Ordering::SeqCst);
    let suppressed = get()
        .query(&[("disableRefresh", "false")])
        .send()
        .await
        .unwrap();
    assert!(suppressed.headers().get("set-cookie").is_none());
    let refreshed = get().send().await.unwrap();
    assert!(
        refreshed.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=86400")
    );
    let refreshed: Value = refreshed.json().await.unwrap();
    assert_eq!(refreshed["session"]["token"], session["token"]);
    assert_eq!(refreshed["session"]["createdAt"], session["createdAt"]);
    assert_ne!(refreshed["session"]["expiresAt"], session["expiresAt"]);
    let post = client
        .post(format!("{base}/api/auth/get-session"))
        .header("cookie", &signed)
        .header("origin", "http://localhost:3000")
        .send()
        .await
        .unwrap();
    assert_eq!(post.status(), 405);
    assert_eq!(post.headers()["cache-control"], "no-store");
    let denied = client
        .post(format!("{base}/api/auth/sign-out"))
        .header("cookie", &signed)
        .header("origin", "https://evil.test")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);
    assert_eq!(get().send().await.unwrap().status(), 200);
    let logout = client
        .post(format!("{base}/api/auth/sign-out"))
        .header("cookie", &signed)
        .header("origin", "http://localhost:3000")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(logout.status(), 200);
    assert_eq!(logout.headers().get_all("set-cookie").iter().count(), 4);
    for header in logout.headers().get_all("set-cookie") {
        assert!(header.to_str().unwrap().contains("Max-Age=0"));
    }
    assert_eq!(
        get().send().await.unwrap().json::<Value>().await.unwrap(),
        Value::Null
    );
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

async fn exercise_http_credentials(client: &reqwest::Client, base: &str) {
    let payload = json!({"name":"TCP User","firstName":"TCP","lastName":"User",
        "email":"tcp-credentials@fixture.test","password":"tcp-test-only-password"});
    let signup = client
        .post(format!("{base}/api/auth/sign-up/email"))
        .header("origin", "http://localhost:3000")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(signup.status(), StatusCode::OK);
    let signup_cookie = signup.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let signup: Value = signup.json().await.unwrap();
    assert_eq!(signup["user"]["email"], payload["email"]);
    assert!(signup["user"].get("password").is_none());
    let view: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", &signup_cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["user"], signup["user"]);
    let signin = client
        .post(format!("{base}/api/auth/sign-in/email"))
        .header("origin", "http://localhost:3000")
        .json(&json!({"email":payload["email"],"password":payload["password"],"rememberMe":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(signin.status(), StatusCode::OK);
    let headers: Vec<_> = signin
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| header.to_str().unwrap())
        .collect();
    assert_eq!(headers.len(), 2);
    assert!(headers.iter().all(|header| !header.contains("Max-Age=")));
    let cookie = headers
        .iter()
        .map(|header| header.split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    let signin: Value = signin.json().await.unwrap();
    assert_eq!(signin["user"], signup["user"]);
    assert_ne!(signin["token"], signup["token"]);
    let changed = client.post(format!("{base}/api/auth/change-password")).header("origin","http://localhost:3000")
        .header("cookie",cookie).json(&json!({"currentPassword":payload["password"],"newPassword":"replacement-tcp-password","revokeOtherSessions":true}))
        .send().await.unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    assert_eq!(changed.headers().get_all("set-cookie").iter().count(), 2);
    assert!(
        changed
            .headers()
            .get_all("set-cookie")
            .iter()
            .all(|header| !header.to_str().unwrap().contains("Max-Age="))
    );
    let changed: Value = changed.json().await.unwrap();
    assert_ne!(changed["token"], signin["token"]);
    assert_eq!(changed["user"], signup["user"]);
    let revoked: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", signup_cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(revoked, Value::Null);
}

#[tokio::test]
async fn live_production_rate_limit_is_atomic_and_shuts_down() {
    let auth = AuthHttp::new(
        Arc::new(MemoryAuthStore::default()),
        "http://localhost:3000".into(),
        b"rate-http-fixture-only-secret-32-characters".to_vec(),
        vec![],
    )
    .unwrap()
    .with_rate_limiting(true)
    .with_clock(|| DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap());
    let router = axum::Router::new().nest("/api/auth", auth.router());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(32));
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let client = client.clone();
        let barrier = barrier.clone();
        requests.spawn(async move {
            barrier.wait().await;
            client
                .post(format!("http://{address}/api/auth/sign-in/email"))
                .header("x-forwarded-for", "198.51.100.209")
                .json(&json!({}))
                .send()
                .await
                .unwrap()
        });
    }
    let mut allowed = 0;
    let mut limited = 0;
    while let Some(response) = requests.join_next().await {
        let response = response.unwrap();
        match response.status() {
            StatusCode::BAD_REQUEST => allowed += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                limited += 1;
                assert_eq!(response.headers()["x-retry-after"], "10");
                assert_eq!(
                    response.json::<Value>().await.unwrap(),
                    json!({"message":"Too many requests. Please try again later."})
                );
            }
            other => panic!("unexpected status {other}"),
        }
    }
    assert_eq!((allowed, limited), (3, 29));
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

async fn exercise_http_email<S: AuthStore>(
    client: &reqwest::Client,
    base: &str,
    store: &Arc<S>,
    secret: &[u8],
    now: DateTime<Utc>,
) {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let sign = |claims: Value| {
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"HS256"}"#),
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
        mac.update(input.as_bytes());
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
        )
    };
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"email-http-owner","email":"email-http@fixture.test","name":"Email User","emailVerified":false,"privateSecret":"retained"})).await.unwrap();
    tx.commit().await.unwrap();
    let token = sign(json!({"email":"email-http@fixture.test", "exp":now.timestamp()+60}));
    let response = client
        .get(format!("{base}/api/auth/verify-email?token={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"status":true,"user":null})
    );
    let forged = client
        .get(format!("{base}/api/auth/verify-email?token={token}x"))
        .send()
        .await
        .unwrap();
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
    let token = sign(
        json!({"email":"email-http@fixture.test", "updateTo":"Changed-Email-Http@fixture.test", "requestType":"change-email-verification", "exp":now.timestamp()+60}),
    );
    let response = client
        .get(format!("{base}/api/auth/verify-email?token={token}"))
        .header("x-forwarded-for", "198.51.100.66")
        .header("user-agent", "EmailHttp")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["user"]["email"], "changed-email-http@fixture.test");
    assert_eq!(body["user"]["emailVerified"], true);
    assert!(body["user"].get("privateSecret").is_none());
    let view: Value = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(view["user"]["email"], body["user"]["email"]);
    assert_eq!(view["session"]["ipAddress"], "198.51.100.66");
    assert_eq!(view["session"]["userAgent"], "EmailHttp");
    let mut tx = store.begin(false).await.unwrap();
    assert_eq!(
        tx.get(Model::User, "email-http-owner")
            .await
            .unwrap()
            .unwrap()["privateSecret"],
        "retained"
    );
    tx.commit().await.unwrap();
}
