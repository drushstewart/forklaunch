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

#[tokio::test]
async fn no_hook_otp_enrollment_checks_session_then_password_then_configuration() {
    let origin = "http://auth.fixture.test";
    let mut browser = Browser {
        router: AuthHttp::new(
            Arc::new(MemoryAuthStore::default()),
            origin.into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_rate_limiting(false)
        .with_two_factor()
        .unwrap()
        .router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    let response = browser
        .call(
            "/two-factor/enable",
            Some(json!({"method":"otp","password":PASSWORD})),
        )
        .await;
    assert_eq!(
        (response.0, response.1),
        (401, json!({"code":"UNAUTHORIZED","message":"Unauthorized"}))
    );
    assert_eq!(
        browser
            .call(
                "/sign-up/email",
                Some(json!({"name":"OTP","firstName":"OTP","lastName":"User","email":"otp-order@example.test","password":PASSWORD}))
            )
            .await
            .0,
        200
    );
    for (password, code, message) in [
        ("wrong", "INVALID_PASSWORD", "Invalid password"),
        (PASSWORD, "OTP_NOT_CONFIGURED", "OTP is not available"),
    ] {
        let response = browser
            .call(
                "/two-factor/enable",
                Some(json!({"method":"otp","password":password})),
            )
            .await;
        assert_eq!(
            (response.0, response.1),
            (400, json!({"code":code,"message":message}))
        );
        assert!(response.2.get("set-cookie").is_none());
    }
}

#[tokio::test]
async fn http_otp_options_are_trusted_and_reach_delivery() {
    let origin = "http://auth.fixture.test";
    let delivery = Arc::new(otp::Delivery::default());
    let mut browser = Browser {
        router: AuthHttp::new(
            Arc::new(MemoryAuthStore::default()),
            origin.into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_rate_limiting(false)
        .with_two_factor_otp_options(
            delivery.clone(),
            OtpOptions::new(8, std::time::Duration::from_millis(1500), 1).unwrap(),
        )
        .unwrap()
        .with_two_factor()
        .unwrap()
        .router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    assert_eq!(
        browser
            .call(
                "/sign-up/email",
                Some(json!({"name":"OTP","firstName":"OTP","lastName":"User","email":"otp-options@example.test","password":PASSWORD}))
            )
            .await
            .0,
        200
    );
    assert_eq!(
        browser
            .call(
                "/two-factor/send-otp",
                Some(json!({"digits":1,"allowedAttempts":100}))
            )
            .await
            .0,
        200
    );
    assert_eq!(delivery.last().len(), 8);
    assert_eq!(
        browser
            .call("/two-factor/verify-otp", Some(json!({"code":"invalid"})))
            .await
            .1["code"],
        "INVALID_CODE"
    );
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-otp",
                Some(json!({"code":delivery.last()}))
            )
            .await
            .1["code"],
        "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_totp_options_preserve_otp_in_both_builder_orders() {
    let now = DateTime::<Utc>::from_timestamp_millis(1_800_000_000_123).unwrap();
    let origin = "http://auth.fixture.test";
    for totp_first in [true, false] {
        let store = Arc::new(MemoryAuthStore::default());
        let delivery = Arc::new(otp::Delivery::default());
        let totp =
            TotpOptions::new(TotpDigits::Eight, std::time::Duration::from_millis(2007)).unwrap();
        let otp = OtpOptions::new(4, std::time::Duration::from_millis(1500), 1).unwrap();
        let app = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || now)
            .with_rate_limiting(false);
        let app = if totp_first {
            app.with_two_factor_totp_options(totp)
                .unwrap()
                .with_two_factor_otp_options(delivery.clone(), otp)
                .unwrap()
        } else {
            app.with_two_factor_otp_options(delivery.clone(), otp)
                .unwrap()
                .with_two_factor_totp_options(totp)
                .unwrap()
        };
        let mut browser = Browser {
            router: app
                .with_two_factor_lifecycle_options(TwoFactorLifecycleOptions::default())
                .unwrap()
                .with_two_factor()
                .unwrap()
                .router(),
            jar: BTreeMap::new(),
            origin: origin.into(),
        };
        assert_eq!(browser.call("/sign-up/email",Some(json!({"name":"Factor","firstName":"Factor","lastName":"User","email":"totp-options@example.test","password":PASSWORD}))).await.0,200);
        assert_eq!(
            browser
                .call("/two-factor/send-otp", Some(json!({})))
                .await
                .0,
            200
        );
        assert_eq!(delivery.last().len(), 4);
        assert_eq!(
            browser
                .call("/two-factor/verify-otp", Some(json!({"code":"invalid"})))
                .await
                .1["code"],
            "INVALID_CODE"
        );
        assert_eq!(
            browser
                .call(
                    "/two-factor/verify-otp",
                    Some(json!({"code":delivery.last()}))
                )
                .await
                .1["code"],
            "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE"
        );
        let enrollment = browser
            .call(
                "/two-factor/enable",
                Some(json!({"password":PASSWORD,"digits":6,"period":30,"skipVerificationOnEnable":true})),
            )
            .await;
        assert_eq!(enrollment.0, 200);
        let uri = url::Url::parse(enrollment.1["totpURI"].as_str().unwrap()).unwrap();
        let pairs: BTreeMap<_, _> = uri.query_pairs().into_owned().collect();
        assert_eq!(pairs["digits"], "8");
        assert_eq!(pairs["period"], "2.007");
        let get = browser
            .call(
                "/two-factor/get-totp-uri",
                Some(json!({"password":PASSWORD})),
            )
            .await;
        assert_eq!(get.0, 200);
        assert_eq!(get.1["totpURI"], enrollment.1["totpURI"]);
        let row = rows(store.as_ref(), Model::TwoFactor).await.remove(0);
        assert_eq!(row["verified"], false);
        assert!(!enabled(&rows(store.as_ref(), Model::User).await[0]).unwrap());
        let code = crypto::code(
            &secret(&row).unwrap(),
            totp.counter(now.timestamp_millis()).unwrap(),
            8,
        );
        assert_eq!(
            browser
                .call("/two-factor/verify-totp", Some(json!({"code":code})))
                .await
                .0,
            200
        );
        assert!(rows(store.as_ref(), Model::User).await[0]["twoFactorEnabled"] == true);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_lifecycle_policy_password_order_skip_issuers_and_disabled_recovery() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let policy = TwoFactorLifecycleOptions::default()
        .with_passwordless(true)
        .with_skip_verification_on_enable(true)
        .with_issuers(Some("Global issuer".into()), Some("URI issuer".into()))
        .unwrap();
    let build = |policy: TwoFactorLifecycleOptions| {
        AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || now)
            .with_rate_limiting(false)
            .with_two_factor_lifecycle_options(policy)
            .unwrap()
    };
    let mut browser = Browser {
        router: build(policy.clone()).router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    assert_eq!(
        browser.call("/two-factor/enable", Some(json!({}))).await.1["code"],
        "UNAUTHORIZED"
    );
    let signup=browser.call("/sign-up/email",Some(json!({"name":"Policy","firstName":"Policy","lastName":"User","email":"policy@example.test","password":PASSWORD}))).await;
    assert_eq!(signup.0, 200);
    assert_eq!(
        browser.call("/two-factor/enable", Some(json!({}))).await.1["code"],
        "INVALID_PASSWORD"
    );
    assert_eq!(
        browser
            .call("/two-factor/enable", Some(json!({"password":"wrong"})))
            .await
            .1["code"],
        "INVALID_PASSWORD"
    );
    let old = browser.jar["better-auth.session_token"].clone();
    let enrollment = browser
        .call(
            "/two-factor/enable",
            Some(json!({"password":PASSWORD,"issuer":""})),
        )
        .await;
    assert_eq!(enrollment.0, 200);
    assert!(
        enrollment.1["totpURI"]
            .as_str()
            .unwrap()
            .contains("Global+issuer")
    );
    assert_ne!(browser.jar["better-auth.session_token"], old);
    let view = browser.call("/get-session", None).await;
    assert_eq!(view.1["user"]["twoFactorEnabled"], true);
    let uri = browser
        .call(
            "/two-factor/get-totp-uri",
            Some(json!({"password":PASSWORD})),
        )
        .await;
    assert_eq!(uri.0, 200);
    assert!(uri.1["totpURI"].as_str().unwrap().contains("URI+issuer"));
    // Removing the credential is owned fixture setup, not a public bypass API.
    let mut tx = store.begin(true).await.unwrap();
    let account = crate::accounts::find_credential(&mut tx, view.1["user"]["id"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    tx.delete(Model::Account, text(&account, "id").unwrap())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        browser
            .call("/two-factor/get-totp-uri", Some(json!({})))
            .await
            .0,
        200
    );
    let schema = browser.call("/open-api/generate-schema", None).await.1;
    assert_eq!(
        schema["paths"]["/two-factor/enable"]["post"]["requestBody"]["content"]["application/json"]
            ["schema"]["required"],
        json!([])
    );
    browser.router = build(
        policy
            .clone()
            .with_uri_passwordless(Some(false))
            .with_backup_passwordless(Some(false)),
    )
    .router();
    assert_eq!(
        browser
            .call("/two-factor/get-totp-uri", Some(json!({})))
            .await
            .1["code"],
        "VALIDATION_ERROR"
    );
    assert_eq!(
        browser
            .call("/two-factor/generate-backup-codes", Some(json!({})))
            .await
            .1["code"],
        "VALIDATION_ERROR"
    );
    browser.router = build(policy.with_totp_disabled(true)).router();
    assert_eq!(
        browser
            .call(
                "/two-factor/get-totp-uri",
                Some(json!({"password":"wrong"}))
            )
            .await
            .1["code"],
        "TOTP_NOT_CONFIGURED"
    );
    let jar = std::mem::take(&mut browser.jar);
    let disabled = browser
        .call("/two-factor/verify-totp", Some(json!({"code":"123456"})))
        .await;
    assert_eq!(
        disabled.1,
        json!({"code":"TOTP_NOT_CONFIGURED","message":"totp isn't configured"})
    );
    assert!(disabled.2.get("set-cookie").is_none());
    assert_eq!(
        browser
            .call("/two-factor/get-totp-uri", Some(json!({})))
            .await
            .1["code"],
        "UNAUTHORIZED"
    );
    browser.jar = jar;
    assert_eq!(
        browser.call("/two-factor/disable", Some(json!({}))).await.0,
        200
    );
    assert_eq!(
        browser.call("/get-session", None).await.1["user"]["twoFactorEnabled"],
        false
    );
}

#[tokio::test]
async fn http_lifetime_options_cookie_and_record_alignment() {
    use std::time::Duration as StdDuration;
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let options =
        TwoFactorLifetimeOptions::new(StdDuration::from_secs(2), StdDuration::from_secs(7))
            .unwrap();
    let app = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_lifetime_options(options)
        .unwrap()
        .with_two_factor_lifecycle_options(
            TwoFactorLifecycleOptions::default().with_skip_verification_on_enable(true),
        )
        .unwrap()
        .with_two_factor_totp_options(TotpOptions::default())
        .unwrap();
    let mut browser = Browser {
        router: app.router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    let credentials = json!({"name":"TTL","firstName":"TTL","lastName":"Test","email":"ttl@example.test","password":PASSWORD});
    assert_eq!(
        browser
            .call("/sign-up/email", Some(credentials.clone()))
            .await
            .0,
        200
    );
    let enrolled = browser
        .call("/two-factor/enable", Some(json!({"password":PASSWORD})))
        .await;
    assert_eq!(enrolled.0, 200);
    browser.call("/sign-out", Some(json!({}))).await;
    let challenge = browser
        .call("/sign-in/email", Some(credentials.clone()))
        .await;
    assert_eq!(challenge.1["twoFactorRedirect"], true);
    assert!(challenge.2.get_all("set-cookie").iter().any(|v| {
        let s = v.to_str().unwrap();
        s.starts_with("better-auth.two_factor=") && s.contains("Max-Age=2")
    }));
    let proof = browser
        .call(
            "/two-factor/verify-backup-code",
            Some(json!({"code":enrolled.1["backupCodes"][0],"trustDevice":true})),
        )
        .await;
    assert_eq!(proof.0, 200);
    assert!(proof.2.get_all("set-cookie").iter().any(|v| {
        let s = v.to_str().unwrap();
        s.starts_with("better-auth.trust_device=") && s.contains("Max-Age=7")
    }));
    let records = rows(store.as_ref(), Model::Verification).await;
    let trust = records
        .iter()
        .find(|r| {
            r["identifier"]
                .as_str()
                .unwrap()
                .starts_with("trust-device-")
        })
        .unwrap();
    assert_eq!(
        time(trust, "expiresAt").unwrap(),
        now + Duration::seconds(7)
    );
    browser.call("/sign-out", Some(json!({}))).await;
    let renewed = browser.call("/sign-in/email", Some(credentials)).await;
    assert!(renewed.1.get("twoFactorRedirect").is_none());
    assert!(renewed.2.get_all("set-cookie").iter().any(|v| {
        let s = v.to_str().unwrap();
        s.starts_with("better-auth.trust_device=") && s.contains("Max-Age=7")
    }));
}

#[tokio::test]
async fn http_account_lockout_options_status_precedence_and_builder_preservation() {
    use std::time::Duration as StdDuration;
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let mut user =
        json!({"id":"http-lockout-user","email":"lockout@example.test","twoFactorEnabled":false});
    let session = new_session("http-lockout-user", "", "", now).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User, user.clone()).await.unwrap();
    tx.insert(Model::Session, session.clone()).await.unwrap();
    tx.commit().await.unwrap();
    let factor = TwoFactorService::new(store.clone(), vec![42; 32], "Lockout".into())
        .unwrap()
        .with_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        );
    let enrolled = factor
        .enable(text(&session, "token").unwrap(), String::new(), None, now)
        .await
        .unwrap();
    user["twoFactorEnabled"] = json!(true);
    let mut tx = store.begin(true).await.unwrap();
    let signin::SigninGate::Challenge(challenge) = factor
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    let app = |enabled| {
        AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || now)
            .with_rate_limiting(false)
            .with_two_factor_account_lockout_options(
                AccountLockoutOptions::new(enabled, 1, StdDuration::from_millis(1500)).unwrap(),
            )
            .unwrap()
            .with_two_factor_lifetime_options(TwoFactorLifetimeOptions::default())
            .unwrap()
            .with_two_factor_totp_options(TotpOptions::default())
            .unwrap()
    };
    let mut browser = Browser {
        router: app(true).router(),
        jar: BTreeMap::from([(
            "better-auth.two_factor".into(),
            crate::cookies::sign_cookie_value(challenge.identifier(), &[42; 32]),
        )]),
        origin: origin.into(),
    };
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-backup-code",
                Some(json!({"code":"invalid"}))
            )
            .await
            .1["code"],
        "INVALID_BACKUP_CODE"
    );
    let locked = browser
        .call(
            "/two-factor/verify-backup-code",
            Some(json!({"code":enrolled.body()["backupCodes"][0]})),
        )
        .await;
    assert_eq!(locked.0, 429);
    assert_eq!(locked.1["code"], "ACCOUNT_TEMPORARILY_LOCKED");
    assert!(locked.2.get("set-cookie").is_none());
    browser.router = app(false).router();
    let success = browser
        .call(
            "/two-factor/verify-backup-code",
            Some(json!({"code":enrolled.body()["backupCodes"][0]})),
        )
        .await;
    assert_eq!(success.0, 200);
    let row = rows(store.as_ref(), Model::TwoFactor).await.remove(0);
    assert_eq!(row["failedVerificationCount"], 1);
    assert_eq!(
        time(&row, "lockedUntil").unwrap(),
        now + Duration::milliseconds(1500)
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_backup_generation_server_policy_and_sanitized_failure() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let store = Arc::new(MemoryAuthStore::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let mode = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let selected = mode.clone();
    let generator: Arc<dyn BackupCodeGenerator> = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        match selected.load(Ordering::SeqCst) {
            1 => Err(error(
                "private-generator",
                "private credential must never escape",
            )),
            2 => Ok(vec!["x".into(); 101]),
            _ => Ok(vec![
                "duplicate".into(),
                "duplicate".into(),
                String::new(),
                "é🔑".into(),
            ]),
        }
    });
    let origin = "http://auth.fixture.test";
    let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_backup_code_options(BackupCodeOptions::new(1, 1).unwrap())
        .unwrap()
        .with_two_factor_backup_code_generator(generator)
        .unwrap()
        .with_two_factor_totp_options(TotpOptions::default())
        .unwrap()
        .with_two_factor_lifetime_options(TwoFactorLifetimeOptions::default())
        .unwrap()
        .with_two_factor_account_lockout_options(AccountLockoutOptions::default())
        .unwrap()
        .with_two_factor_lifecycle_options(
            TwoFactorLifecycleOptions::default().with_skip_verification_on_enable(true),
        )
        .unwrap()
        .router();
    let mut browser = Browser {
        router,
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    assert_eq!(
        browser
            .call("/two-factor/enable", Some(json!({"password":PASSWORD})))
            .await
            .0,
        401
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(browser.call("/sign-up/email", Some(json!({"name":"Backup", "firstName":"Backup", "lastName":"User", "email":"backup-http@example.test", "password":PASSWORD}))).await.0, 200);
    assert_eq!(
        browser
            .call("/two-factor/enable", Some(json!({"password":"wrong"})))
            .await
            .0,
        400
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let (status, body, _) = browser.call("/two-factor/enable", Some(json!({"password":PASSWORD, "backupCodeOptions":{"amount":100,"length":127}, "customBackupCodesGenerate":["attacker"]}))).await;
    assert_eq!(status, 200);
    assert_eq!(
        body["backupCodes"],
        json!(["duplicate", "duplicate", "", "é🔑"])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let factors = rows(store.as_ref(), Model::TwoFactor).await;
    let sessions = rows(store.as_ref(), Model::Session).await;
    for invalid in [1, 2] {
        mode.store(invalid, Ordering::SeqCst);
        let (status, body, headers) = browser
            .call(
                "/two-factor/generate-backup-codes",
                Some(json!({"password":PASSWORD})),
            )
            .await;
        assert_eq!(status, 500);
        assert!(body.is_null());
        assert!(headers.get("set-cookie").is_none());
        assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, factors);
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
    }
    mode.store(0, Ordering::SeqCst);
    let (status, body, _) = browser
        .call(
            "/two-factor/generate-backup-codes",
            Some(json!({"password":PASSWORD})),
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["backupCodes"],
        json!(["duplicate", "duplicate", "", "é🔑"])
    );
}

#[tokio::test]
async fn http_otp_storage_policy_preserved_and_invalid_inputs_counted() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let session = new_session("otp-codec-http", "", "", now).unwrap();
    let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"otp-codec-http","email":"codec@example.test","twoFactorEnabled":true}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(otp::Delivery::default());
    let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_otp_storage(OtpStorage::Hashed)
        .unwrap()
        .with_two_factor_otp(delivery.clone())
        .unwrap()
        .with_two_factor_totp_options(TotpOptions::default())
        .unwrap()
        .with_two_factor_backup_code_options(BackupCodeOptions::default())
        .unwrap()
        .with_two_factor_lifecycle_options(TwoFactorLifecycleOptions::default())
        .unwrap()
        .router();
    let mut browser = Browser {
        router,
        origin: origin.into(),
        jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
    };
    assert_eq!(
        browser
            .call("/two-factor/send-otp", Some(json!({"storeOTP":"plain"})))
            .await
            .0,
        200
    );
    let code = delivery.last();
    let saved = rows(store.as_ref(), Model::Verification).await;
    assert_eq!(
        saved[0]["value"],
        json!(OtpStorage::Hashed.comparison(&code).as_str())
    );
    for supplied in ["", "é🔑", "not-digits"] {
        let (status, body, _) = browser
            .call("/two-factor/verify-otp", Some(json!({"code":supplied})))
            .await;
        assert_eq!(status, 401);
        assert_eq!(body["code"], "INVALID_CODE");
    }
    let before = rows(store.as_ref(), Model::Verification).await;
    assert_eq!(before[0]["attempts"], 3);
    assert_eq!(
        browser
            .call("/two-factor/verify-otp", Some(json!({"code":123})))
            .await
            .0,
        400
    );
    assert_eq!(rows(store.as_ref(), Model::Verification).await, before);
    assert_eq!(
        browser
            .call("/two-factor/verify-otp", Some(json!({"code":code})))
            .await
            .0,
        200
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_encrypted_otp_failure_consumes_without_session_or_cookie() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let session = new_session("encrypted-http-user", "", "", now).unwrap();
    let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"encrypted-http-user","email":"encrypted-http@example.test","twoFactorEnabled":true})).await.unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(otp::Delivery::default());
    let keys = Arc::new(OtpEncryptionKeys::versioned(7, vec![(7, "x".repeat(32))], None).unwrap());
    let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_otp_encryption_keys(keys)
        .unwrap()
        .with_two_factor_otp(delivery.clone())
        .unwrap()
        .with_two_factor_backup_code_options(BackupCodeOptions::default())
        .unwrap()
        .with_two_factor_lifetime_options(TwoFactorLifetimeOptions::default())
        .unwrap()
        .router();
    let mut browser = Browser {
        router,
        origin: origin.into(),
        jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
    };
    let sessions = rows(store.as_ref(), Model::Session).await;
    for kind in 0..4 {
        assert_eq!(
            browser
                .call("/two-factor/send-otp", Some(json!({"storeOTP":"plain"})))
                .await
                .0,
            200
        );
        let mut saved = rows(store.as_ref(), Model::Verification).await.remove(0);
        let encrypted = text(&saved, "value").unwrap();
        assert!(encrypted.starts_with("$ba$7$"));
        saved["value"] = json!(match kind {
            0 => "$ba$7$not-hex".to_owned(),
            1 => encrypted.replace("$7$", "$8$"),
            2 => encrypted.replace("$7$", "$-1$"),
            _ => format!(
                "{}{}",
                &encrypted[..encrypted.len() - 1],
                if encrypted.ends_with('0') { '1' } else { '0' }
            ),
        });
        let mut tx = store.begin(true).await.unwrap();
        tx.replace(Model::Verification, saved).await.unwrap();
        tx.commit().await.unwrap();
        let (status, body, headers) = browser
            .call(
                "/two-factor/verify-otp",
                Some(json!({"code":delivery.last()})),
            )
            .await;
        assert_eq!(status, 500);
        assert!(body.is_null());
        assert!(headers.get("set-cookie").is_none());
        assert!(headers.get("set-auth-jwt").is_none());
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
        assert!(rows(store.as_ref(), Model::Verification).await.is_empty());
        assert_eq!(
            browser
                .call(
                    "/two-factor/verify-otp",
                    Some(json!({"code":delivery.last()}))
                )
                .await
                .1["code"],
            "OTP_HAS_EXPIRED"
        );
    }
    assert_eq!(
        browser
            .call("/two-factor/send-otp", Some(json!({})))
            .await
            .0,
        200
    );
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-otp",
                Some(json!({"code":delivery.last()}))
            )
            .await
            .0,
        200
    );
}

#[tokio::test]
async fn http_custom_otp_codec_error_is_empty_without_authentication_headers() {
    struct Codec;
    impl OtpEncryptionCodec for Codec {
        fn encrypt(&self, input: OtpCodecText) -> OtpCodecFuture {
            Box::pin(async move { Ok(input) })
        }
        fn decrypt(&self, _: OtpCodecText) -> OtpCodecFuture {
            Box::pin(async { Err(error("private-codec-secret", "never expose callback data")) })
        }
    }
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let session = new_session("custom-http-user", "", "", now).unwrap();
    let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"custom-http-user","email":"custom-http@example.test","twoFactorEnabled":true}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let delivery = Arc::new(otp::Delivery::default());
    let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_otp_encryption_codec(Arc::new(Codec), OtpCodecOptions::default())
        .unwrap()
        .with_two_factor_otp(delivery.clone())
        .unwrap()
        .with_two_factor_backup_code_options(BackupCodeOptions::default())
        .unwrap()
        .router();
    let mut browser = Browser {
        router,
        origin: origin.into(),
        jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
    };
    assert_eq!(
        browser
            .call("/two-factor/send-otp", Some(json!({"storeOTP":"plain"})))
            .await
            .0,
        200
    );
    let before = rows(store.as_ref(), Model::Session).await;
    let (status, body, headers) = browser
        .call(
            "/two-factor/verify-otp",
            Some(json!({"code":delivery.last()})),
        )
        .await;
    assert_eq!(status, 500);
    assert!(body.is_null());
    assert!(headers.get("set-cookie").is_none());
    assert!(headers.get("set-auth-jwt").is_none());
    assert_eq!(rows(store.as_ref(), Model::Session).await, before);
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-otp",
                Some(json!({"code":delivery.last()}))
            )
            .await
            .1["code"],
        "OTP_HAS_EXPIRED"
    );
}

#[tokio::test]
async fn http_otp_builtin_clock_is_sampled_once_and_custom_clock_stays_fresh() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Codec;
    impl OtpHashCodec for Codec {
        fn hash(&self, input: OtpCodecText) -> OtpCodecFuture {
            Box::pin(async move { Ok(input) })
        }
    }
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    for mode in 0..4 {
        let store = Arc::new(MemoryAuthStore::default());
        let session = new_session("clock-user", "", "", now).unwrap();
        let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(
            Model::User,
            json!({"id":"clock-user","email":"clock@example.test","twoFactorEnabled":true}),
        )
        .await
        .unwrap();
        tx.insert(Model::Session, session).await.unwrap();
        tx.commit().await.unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let clock_count = count.clone();
        let delivery = Arc::new(otp::Delivery::default());
        let app = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || {
                let tick = clock_count.fetch_add(1, Ordering::SeqCst);
                now + Duration::milliseconds(i64::try_from(tick).unwrap())
            })
            .with_rate_limiting(false)
            .with_two_factor_otp(delivery.clone())
            .unwrap();
        let app = match mode {
            0 => app.with_two_factor_otp_storage(OtpStorage::Plain).unwrap(),
            1 => app.with_two_factor_otp_storage(OtpStorage::Hashed).unwrap(),
            2 => app
                .with_two_factor_otp_encryption_keys(Arc::new(
                    OtpEncryptionKeys::versioned(7, vec![(7, "x".repeat(32))], None).unwrap(),
                ))
                .unwrap(),
            _ => app
                .with_two_factor_otp_hash_codec(Arc::new(Codec), OtpCodecOptions::default())
                .unwrap(),
        };
        let mut browser = Browser {
            router: app.router(),
            origin: origin.into(),
            jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
        };
        assert_eq!(
            browser
                .call("/two-factor/send-otp", Some(json!({})))
                .await
                .0,
            200
        );
        let calls = count.load(Ordering::SeqCst);
        let saved = rows(store.as_ref(), Model::Verification).await.remove(0);
        if mode < 3 {
            assert_eq!(calls, 1);
            assert_eq!(
                time(&saved, "expiresAt").unwrap(),
                now + Duration::seconds(180)
            );
        } else {
            assert!(calls > 1);
            assert!(time(&saved, "expiresAt").unwrap() > now + Duration::seconds(180));
        }
        assert_eq!(
            browser
                .call(
                    "/two-factor/verify-otp",
                    Some(json!({"code": delivery.last()}))
                )
                .await
                .0,
            200
        );
        let verify_calls = count.load(Ordering::SeqCst) - calls;
        if mode < 3 {
            assert_eq!(verify_calls, 1);
        } else {
            assert!(verify_calls > 1);
        }
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_backup_storage_default_explicit_preservation_and_failure_projection() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    for mode in 0..4 {
        let store = Arc::new(MemoryAuthStore::default());
        let session = new_session("backup-http-codec", "", "", now).unwrap();
        let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
        let mut tx = store.begin(true).await.unwrap();
        tx.insert(Model::User,json!({"id":"backup-http-codec","email":"backup-http-codec@example.test","twoFactorEnabled":false})).await.unwrap();
        tx.insert(Model::Session, session).await.unwrap();
        tx.commit().await.unwrap();
        let app = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_clock(move || now)
            .with_rate_limiting(false);
        let app = match mode {
            1 => app
                .with_two_factor_backup_code_storage(BackupCodeStorage::LegacyArray)
                .unwrap(),
            2 => app
                .with_two_factor_backup_code_storage(BackupCodeStorage::PlainJson)
                .unwrap(),
            3 => app
                .with_two_factor_backup_code_encryption_keys(Arc::new(
                    BackupCodeEncryptionKeys::versioned(9, vec![(9, "z".repeat(32))], None)
                        .unwrap(),
                ))
                .unwrap(),
            _ => app.with_two_factor().unwrap(),
        }
        .with_two_factor_otp_storage(OtpStorage::Hashed)
        .unwrap()
        .with_two_factor_totp_options(TotpOptions::default())
        .unwrap()
        .with_two_factor_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        )
        .unwrap()
        .with_two_factor_backup_code_generator(Arc::new(|| {
            Ok(vec!["owned-code".into(), "second".into()])
        }))
        .unwrap()
        .with_two_factor()
        .unwrap();
        let mut browser = Browser {
            router: app.router(),
            origin: origin.into(),
            jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
        };
        let (status, body, _) = browser
            .call(
                "/two-factor/enable",
                Some(json!({"storeBackupCodes":"plain"})),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(body["backupCodes"], json!(["owned-code", "second"]));
        let row = rows(store.as_ref(), Model::TwoFactor).await.remove(0);
        match mode {
            1 => assert_eq!(row["backupCodes"], body["backupCodes"]),
            2 => assert_eq!(row["backupCodes"], json!("[\"owned-code\",\"second\"]")),
            3 => assert!(row["backupCodes"].as_str().unwrap().starts_with("$ba$9$")),
            _ => {
                assert!(row["backupCodes"].is_string());
                assert!(!row["backupCodes"].as_str().unwrap().contains("owned-code"));
            }
        }
        assert_eq!(
            browser
                .call(
                    "/two-factor/verify-backup-code",
                    Some(json!({"code":"owned-code"}))
                )
                .await
                .0,
            200
        );
        let saved = rows(store.as_ref(), Model::TwoFactor).await.remove(0);
        let mut broken = saved.clone();
        broken["backupCodesFormat"] = json!(999);
        let mut tx = store.begin(true).await.unwrap();
        tx.replace(Model::TwoFactor, broken.clone()).await.unwrap();
        tx.commit().await.unwrap();
        let sessions = rows(store.as_ref(), Model::Session).await;
        let (status, body, headers) = browser
            .call(
                "/two-factor/verify-backup-code",
                Some(json!({"code":"second"})),
            )
            .await;
        assert_eq!(status, 500);
        assert!(body.is_null());
        assert!(headers.get("set-cookie").is_none());
        assert!(headers.get("set-auth-jwt").is_none());
        assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, [broken]);
        assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn http_async_backup_lifecycle_faults_and_default_builder_preservation() {
    use std::sync::atomic::{AtomicU8, Ordering};
    struct Codec(Arc<AtomicU8>);
    impl BackupCodeEncryptionCodec for Codec {
        fn encrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            let fail = self.0.load(Ordering::SeqCst) == 1;
            Box::pin(async move {
                if fail {
                    Err(error("private-backup-error", "secret must never escape"))
                } else {
                    BackupCodeCodecText::new(format!("X{}", input.as_str()))
                }
            })
        }
        fn decrypt(&self, input: BackupCodeCodecText) -> BackupCodeCodecFuture {
            let fail = self.0.load(Ordering::SeqCst) == 2;
            Box::pin(async move {
                if fail {
                    Err(error("private-backup-error", "secret must never escape"))
                } else {
                    BackupCodeCodecText::new(input.as_str()[1..].into())
                }
            })
        }
    }
    let store = Arc::new(MemoryAuthStore::default());
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let origin = "http://auth.fixture.test";
    let session = new_session("async-http-user", "", "", now).unwrap();
    let cookie = crate::cookies::sign_cookie_value(text(&session, "token").unwrap(), &[42; 32]);
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"async-http-user","email":"async-http@example.test","twoFactorEnabled":false}),
    )
    .await
    .unwrap();
    tx.insert(Model::Session, session).await.unwrap();
    tx.commit().await.unwrap();
    let mode = Arc::new(AtomicU8::new(0));
    let router = AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
        .unwrap()
        .with_clock(move || now)
        .with_rate_limiting(false)
        .with_two_factor_backup_code_encryption_codec(
            Arc::new(Codec(mode.clone())),
            BackupCodeCodecOptions::default(),
        )
        .unwrap()
        .with_two_factor_totp_options(TotpOptions::default())
        .unwrap()
        .with_two_factor_otp_storage(OtpStorage::Hashed)
        .unwrap()
        .with_two_factor_lifecycle_options(
            TwoFactorLifecycleOptions::default()
                .with_passwordless(true)
                .with_skip_verification_on_enable(true),
        )
        .unwrap()
        .with_two_factor_backup_code_generator(Arc::new(|| Ok(vec!["one".into(), "two".into()])))
        .unwrap()
        .router();
    let mut browser = Browser {
        router,
        origin: origin.into(),
        jar: BTreeMap::from([("better-auth.session_token".into(), cookie)]),
    };
    assert_eq!(
        browser
            .call(
                "/two-factor/enable",
                Some(json!({"storeBackupCodes":"plain"}))
            )
            .await
            .0,
        200
    );
    let row = rows(store.as_ref(), Model::TwoFactor).await.remove(0);
    assert_eq!(row["backupCodesFormat"], 2);
    assert_eq!(row["backupCodes"], "X[\"one\",\"two\"]");
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-backup-code",
                Some(json!({"code":"one"}))
            )
            .await
            .0,
        200
    );
    let before = rows(store.as_ref(), Model::TwoFactor).await;
    mode.store(2, Ordering::SeqCst);
    let (status, body, headers) = browser
        .call(
            "/two-factor/verify-backup-code",
            Some(json!({"code":"two"})),
        )
        .await;
    assert_eq!(status, 500);
    assert!(body.is_null());
    assert!(headers.get("set-cookie").is_none());
    assert!(headers.get("set-auth-jwt").is_none());
    assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
    let gate = TwoFactorService::new(store.clone(), vec![42; 32], "Gate".into()).unwrap();
    let mut tx = store.begin(true).await.unwrap();
    let user = tx
        .get(Model::User, "async-http-user")
        .await
        .unwrap()
        .unwrap();
    let signin::SigninGate::Challenge(challenge) = gate
        .gate(&mut tx, &user, None, true, "", "", now)
        .await
        .unwrap()
    else {
        panic!("challenge")
    };
    tx.commit().await.unwrap();
    browser.jar.remove("better-auth.session_token");
    browser.jar.insert(
        "better-auth.two_factor".into(),
        crate::cookies::sign_cookie_value(challenge.identifier(), &[42; 32]),
    );
    mode.store(1, Ordering::SeqCst);
    let sessions = rows(store.as_ref(), Model::Session).await;
    let (status, body, headers) = browser
        .call(
            "/two-factor/verify-backup-code",
            Some(json!({"code":"two","trustDevice":true})),
        )
        .await;
    assert_eq!(status, 500);
    assert!(body.is_null());
    assert!(headers.get("set-cookie").is_none());
    assert!(headers.get("set-auth-jwt").is_none());
    assert_eq!(rows(store.as_ref(), Model::TwoFactor).await, before);
    assert_eq!(rows(store.as_ref(), Model::Session).await, sessions);
    mode.store(0, Ordering::SeqCst);
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-backup-code",
                Some(json!({"code":"two"}))
            )
            .await
            .0,
        401
    );
}

#[tokio::test]
async fn http_otp_background_returns_before_task_and_preserves_builders() {
    let origin = "http://auth.fixture.test";
    let queue = Arc::new(otp_background::Queue::default());
    let delivery = Arc::new(otp::Delivery::default());
    delivery.mode.store(3, std::sync::atomic::Ordering::SeqCst);
    let mut browser = Browser {
        router: AuthHttp::new(
            Arc::new(MemoryAuthStore::default()),
            origin.into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_rate_limiting(false)
        .with_two_factor_otp_background_dispatcher(queue.clone())
        .unwrap()
        .with_two_factor_otp_options(delivery.clone(), OtpOptions::default())
        .unwrap()
        .with_two_factor_otp_storage(OtpStorage::Hashed)
        .unwrap()
        .router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    assert_eq!(browser.call("/sign-up/email",Some(json!({"name":"Owned","firstName":"Owned","lastName":"User","email":"background-http@example.test","password":PASSWORD}))).await.0,200);
    let response = browser.call("/two-factor/send-otp", Some(json!({}))).await;
    assert_eq!(response.0, 200);
    assert_eq!(response.1, json!({"status":true}));
    assert!(response.2.get("set-auth-jwt").is_none());
    assert!(response.2.get("set-cookie").is_none());
    let task = queue.tasks.lock().unwrap().pop().unwrap();
    let worker = tokio::spawn(task);
    delivery.entered.notified().await;
    assert!(!worker.is_finished());
    delivery.released.notify_one();
    worker.await.unwrap();
    assert_eq!(
        browser
            .call(
                "/two-factor/verify-otp",
                Some(json!({"code":delivery.last()}))
            )
            .await
            .0,
        200
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // Keep the request/callback/consume lifecycle in one fixture.
async fn http_reset_delivery_optin_privacy_redirect_consume_and_shared_dispatch() {
    use crate::accounts::reset_request::tests::Delivery;
    let origin = "http://auth.fixture.test";
    let store = Arc::new(MemoryAuthStore::default());
    let delivery = Arc::new(Delivery::default());
    let queue = Arc::new(otp_background::Queue::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(Model::User,json!({"id":"reset-http","email":"reset-http@example.test","name":"Owned","emailVerified":false,"password":"private"})).await.unwrap();
    tx.commit().await.unwrap();
    let base = || {
        AuthHttp::new(store.clone(), origin.into(), vec![42; 32], vec![])
            .unwrap()
            .with_rate_limiting(false)
    };
    let mut disabled = Browser {
        router: base().router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    assert_eq!(
        disabled
            .call(
                "/request-password-reset",
                Some(json!({"email":"reset-http@example.test"}))
            )
            .await
            .1["code"],
        "RESET_PASSWORD_DISABLED"
    );
    delivery.mode.store(3, std::sync::atomic::Ordering::SeqCst);
    let mut browser = Browser {
        router: base()
            .with_password_reset_delivery(delivery.clone())
            .with_background_dispatcher(queue.clone())
            .router(),
        jar: BTreeMap::new(),
        origin: origin.into(),
    };
    let missing = browser
        .call(
            "/request-password-reset",
            Some(json!({"email":"missing@example.test"})),
        )
        .await;
    assert_eq!(missing.0, 200);
    assert!(delivery.contexts.lock().unwrap().is_empty());
    assert_eq!(
        browser
            .call(
                "/request-password-reset",
                Some(
                    json!({"email":"reset-http@example.test","redirectTo":"https://evil.example/"})
                )
            )
            .await
            .0,
        403
    );
    assert!(delivery.contexts.lock().unwrap().is_empty());
    let response = browser
        .call(
            "/request-password-reset",
            Some(json!({"email":"RESET-HTTP@EXAMPLE.TEST","redirectTo":"/owned"})),
        )
        .await;
    assert_eq!(response.0, 200);
    assert_eq!(response.1, missing.1);
    assert!(response.2.get("set-cookie").is_none());
    assert!(response.2.get("set-auth-jwt").is_none());
    let proof = delivery.contexts.lock().unwrap()[0].token().to_owned();
    assert!(!response.1.to_string().contains(&proof));
    let task = queue.tasks.lock().unwrap().pop().unwrap();
    let worker = tokio::spawn(task);
    delivery.entered.notified().await;
    assert!(!worker.is_finished());
    delivery.release.notify_one();
    worker.await.unwrap();
    let callback = browser
        .call(
            &format!("/reset-password/{proof}?callbackURL=%2Fowned"),
            None,
        )
        .await;
    assert_eq!(callback.0, 302);
    assert!(callback.2["location"].to_str().unwrap().contains(&proof));
    assert_eq!(
        browser
            .call(
                "/reset-password",
                Some(json!({"token":proof,"newPassword":"owned-new-password"}))
            )
            .await
            .0,
        200
    );
    assert_eq!(
        browser
            .call(
                "/reset-password",
                Some(json!({"token":proof,"newPassword":"owned-new-password"}))
            )
            .await
            .0,
        400
    );
    delivery.mode.store(1, std::sync::atomic::Ordering::SeqCst);
    let failure = browser
        .call(
            "/request-password-reset",
            Some(json!({"email":"reset-http@example.test"})),
        )
        .await;
    assert_eq!(failure.0, 500);
    assert!(failure.1.is_null());
}

#[tokio::test]
async fn http_shared_background_preserves_legacy_otp_alias_and_override_order() {
    fn legacy_task(task: OtpBackgroundTask) -> crate::background::AuthBackgroundTask {
        task
    }
    for explicit in [false, true] {
        for global_first in [false, true] {
            let global = Arc::new(otp_background::Queue::default());
            let local = Arc::new(otp_background::Queue::default());
            let delivery = Arc::new(otp::Delivery::default());
            let origin = "http://auth.fixture.test";
            let mut app = AuthHttp::new(
                Arc::new(MemoryAuthStore::default()),
                origin.into(),
                vec![42; 32],
                vec![],
            )
            .unwrap()
            .with_rate_limiting(false);
            if global_first {
                app = app.with_background_dispatcher(global.clone());
            }
            if explicit {
                app = app
                    .with_two_factor_otp_background_dispatcher(local.clone())
                    .unwrap();
            }
            app = app.with_two_factor_otp(delivery).unwrap();
            if !global_first {
                app = app.with_background_dispatcher(global.clone());
            }
            let mut browser = Browser {
                router: app.router(),
                jar: BTreeMap::new(),
                origin: origin.into(),
            };
            assert_eq!(browser.call("/sign-up/email",Some(json!({"name":"Owned","firstName":"Owned","lastName":"User","email":"reset-scheduler@example.test","password":PASSWORD}))).await.0,200);
            assert_eq!(
                browser
                    .call("/two-factor/send-otp", Some(json!({})))
                    .await
                    .0,
                200
            );
            let chosen = if explicit { &local } else { &global };
            let other = if explicit { &global } else { &local };
            assert!(other.tasks.lock().unwrap().is_empty());
            let task = chosen.tasks.lock().unwrap().pop().unwrap();
            legacy_task(task).await;
        }
    }
}
