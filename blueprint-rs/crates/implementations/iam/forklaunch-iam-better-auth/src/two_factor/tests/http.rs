use super::*;
use crate::http::AuthHttp;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request},
};
use std::collections::BTreeMap;
use tower::ServiceExt;

struct Browser {
    router: Router,
    jar: BTreeMap<String, String>,
    origin: String,
}
impl Browser {
    async fn call(&mut self, path: &str, body: Option<Value>) -> (u16, Value, HeaderMap) {
        let mut request = Request::builder()
            .uri(path)
            .method(if body.is_some() { "POST" } else { "GET" })
            .header("origin", &self.origin)
            .header("content-type", "application/json")
            .header("user-agent", "owned-factor-browser")
            .header("x-forwarded-for", "192.0.2.17")
            .header(
                "cookie",
                self.jar
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        if body.is_none() {
            request = request.header("accept", "application/json");
        }
        let response = self
            .router
            .clone()
            .oneshot(
                request
                    .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        for cookie in headers.get_all("set-cookie") {
            let cookie = cookie.to_str().unwrap();
            let (name, value) = cookie.split(';').next().unwrap().split_once('=').unwrap();
            if cookie.contains("Max-Age=0") {
                self.jar.remove(name);
            } else {
                self.jar.insert(name.into(), value.into());
            }
        }
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap()
        };
        (status, value, headers)
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_factor_cookies_authority_and_metadata() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for secure in [false, true] {
        let origin = if secure {
            "https://auth.fixture.test"
        } else {
            "http://auth.fixture.test"
        };
        let store = Arc::new(MemoryAuthStore::default());
        let signing_failure = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_failure = signing_failure.clone();
        let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || now)
            .with_jwt_subject_resolver(move |_| {
                let failed = callback_failure.load(std::sync::atomic::Ordering::SeqCst);
                async move {
                    if failed {
                        Err(BlueprintError::new(
                            "private-callback",
                            "never expose callback internals",
                        ))
                    } else {
                        Ok(None)
                    }
                }
            })
            .with_rate_limiting(false)
            .with_two_factor()
            .unwrap()
            .router();
        let mut browser = Browser {
            router,
            jar: BTreeMap::new(),
            origin: origin.into(),
        };
        let name = format!(
            "{}better-auth.session_token",
            if secure { "__Secure-" } else { "" }
        );
        let challenge_name = format!(
            "{}better-auth.two_factor",
            if secure { "__Secure-" } else { "" }
        );
        let credentials = json!({"name":"Factor","firstName":"Factor","lastName":"User","email":"factor@example.test","password":PASSWORD,"rememberMe":!secure});
        let (status, signup, _) = browser
            .call("/sign-up/email", Some(credentials.clone()))
            .await;
        assert_eq!(status, 200);
        assert_eq!(signup["user"]["twoFactorEnabled"], false);
        let old = browser.jar[&name].clone();
        browser.origin = "https://foreign.fixture.test".into();
        assert_eq!(
            browser
                .call("/two-factor/enable", Some(json!({"password":PASSWORD})))
                .await
                .0,
            403
        );
        browser.origin = origin.into();
        assert_eq!(
            browser
                .call("/two-factor/enable", Some(json!({"password":"wrong"})))
                .await
                .0,
            400
        );
        let (status, enrollment, headers) = browser
            .call("/two-factor/enable", Some(json!({"password":PASSWORD})))
            .await;
        assert_eq!(status, 200);
        assert_eq!(enrollment["backupCodes"].as_array().unwrap().len(), 10);
        assert!(headers.get("set-cookie").is_none());
        let code = factor_code(store.as_ref(), now).await;
        let (status, verified, headers) = browser
            .call("/two-factor/verify-totp", Some(json!({"code":code})))
            .await;
        assert_eq!(status, 200);
        assert_eq!(verified["token"], signup["token"]);
        assert_eq!(verified["user"]["twoFactorEnabled"], false);
        assert_ne!(browser.jar[&name], old);
        if secure {
            let session_cookie = headers
                .get_all("set-cookie")
                .iter()
                .find(|v| v.to_str().unwrap().starts_with(&format!("{name}=")))
                .unwrap();
            assert!(!session_cookie.to_str().unwrap().contains("Max-Age="));
        }
        assert!(
            headers
                .get_all("set-cookie")
                .iter()
                .all(|v| v.to_str().unwrap().contains("HttpOnly"))
        );
        let current = browser.jar[&name].clone();
        browser.jar.insert(name.clone(), old);
        assert!(browser.call("/get-session", None).await.1.is_null());
        browser.jar.insert(name.clone(), current);
        assert_eq!(
            browser.call("/get-session", None).await.1["user"]["twoFactorEnabled"],
            true
        );
        browser.jar.clear();
        let (status, pending, pending_headers) = browser
            .call("/sign-in/email", Some(credentials.clone()))
            .await;
        assert_eq!(status, 200);
        assert_eq!(
            pending,
            json!({"twoFactorRedirect":true,"twoFactorMethods":["totp"]})
        );
        assert!(pending_headers.get("set-auth-jwt").is_none());
        assert!(!browser.jar.contains_key(&name));
        assert!(browser.jar.contains_key(&challenge_name));
        assert!(browser.call("/get-session", None).await.1.is_null());
        signing_failure.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(browser.call("/token", None).await.0, 401);
        let signed = browser.jar[&challenge_name].clone();
        browser
            .jar
            .insert(challenge_name.clone(), format!("{signed}forged"));
        assert_eq!(
            browser
                .call("/two-factor/verify-totp", Some(json!({"code":code})))
                .await
                .0,
            401
        );
        browser.jar.insert(challenge_name.clone(), signed.clone());
        let (status, authenticated, _) = browser
            .call(
                "/two-factor/verify-totp",
                Some(json!({"code":code,"trustDevice":true})),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(authenticated["user"]["twoFactorEnabled"], true);
        assert!(!browser.jar.contains_key(&challenge_name));
        let failed_jwt = browser.call("/get-session", None).await;
        assert_eq!(failed_jwt.0, 500);
        assert!(failed_jwt.1.is_null());
        assert!(failed_jwt.2.get("set-auth-jwt").is_none());
        signing_failure.store(false, std::sync::atomic::Ordering::SeqCst);
        let (_, session, headers) = browser.call("/get-session", None).await;
        let jwt = headers["set-auth-jwt"].to_str().unwrap();
        let claims: Value = serde_json::from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(jwt.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["sessionId"], session["session"]["id"]);
        assert_eq!(session["session"]["ipAddress"], "192.0.2.17");
        assert_eq!(session["session"]["userAgent"], "owned-factor-browser");
        let saved = browser.jar.clone();
        browser.jar.clear();
        browser.jar.insert(challenge_name, signed);
        assert_eq!(
            browser
                .call("/two-factor/verify-totp", Some(json!({"code":code})))
                .await
                .0,
            401
        );
        browser.jar = saved;
        assert_eq!(
            browser
                .call("/two-factor/disable", Some(json!({"password":PASSWORD})))
                .await
                .0,
            200
        );
        assert_eq!(
            browser.call("/get-session", None).await.1["user"]["twoFactorEnabled"],
            false
        );
        assert!(
            browser
                .jar
                .keys()
                .all(|name| !name.ends_with("trust_device"))
        );
    }
}
#[tokio::test]
async fn http_factor_opt_in_routes_rate_rule_and_schema() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let build = || {
        AuthHttp::new(
            Arc::new(MemoryAuthStore::default()),
            "http://auth.fixture.test".into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_clock(move || now)
    };
    let mut browser = Browser {
        router: build().router(),
        jar: BTreeMap::new(),
        origin: "http://auth.fixture.test".into(),
    };
    assert_eq!(
        browser
            .call("/two-factor/verify-totp", Some(json!({"code":"123456"})))
            .await
            .0,
        404
    );
    browser.router = build()
        .with_two_factor()
        .unwrap()
        .with_rate_limiting(true)
        .router();
    for _ in 0..3 {
        assert_eq!(
            browser
                .call("/two-factor/verify-totp", Some(json!({"code":"123456"})))
                .await
                .0,
            401
        );
    }
    let response = browser
        .call("/two-factor/verify-totp", Some(json!({"code":"123456"})))
        .await;
    assert_eq!(response.0, 429);
    assert_eq!(response.2["x-retry-after"], "10");
    let schema = browser.call("/open-api/generate-schema", None).await.1;
    assert!(schema["paths"]["/two-factor/verify-totp"]["post"].is_object());
    assert!(
        build()
            .with_jwks_path("/two-factor/keys")
            .unwrap()
            .with_two_factor()
            .is_err()
    );
    assert!(
        build()
            .with_two_factor()
            .unwrap()
            .with_jwks_path("/two-factor/keys")
            .is_err()
    );
}

#[tokio::test]
async fn disabled_otp_routes_preserve_validation_and_authority_order() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let app = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor()
        .unwrap();
    let router = app.router();
    for (path, body, expected, code) in [
        ("/two-factor/send-otp", None, 400, "OTP_NOT_CONFIGURED"),
        (
            "/two-factor/send-otp",
            Some(json!({"trustDevice":"bad"})),
            400,
            "VALIDATION_ERROR",
        ),
        (
            "/two-factor/verify-otp",
            Some(json!({})),
            400,
            "VALIDATION_ERROR",
        ),
        (
            "/two-factor/verify-otp",
            Some(json!({"code":"123456"})),
            401,
            "INVALID_TWO_FACTOR_COOKIE",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .method("POST")
                    .header("origin", origin)
                    .header("content-type", "application/json")
                    .body(body.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert!(response.headers().get("set-cookie").is_none());
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(value["code"], code);
    }
    let session = new_session("otp-user", "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"otp-user","email":"otp@example.test","emailVerified":false}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let service = TwoFactorService::new(store, vec![42; 32], "Better Auth".into()).unwrap();
    assert_eq!(
        service
            .verify_otp(text(&session, "token").unwrap(), "", "123456", false, now)
            .await
            .err()
            .unwrap()
            .code,
        "OTP_HAS_EXPIRED"
    );
}
