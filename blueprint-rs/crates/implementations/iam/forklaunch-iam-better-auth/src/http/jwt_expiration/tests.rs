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
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
struct Signer(Arc<Mutex<Vec<Value>>>);
impl JwtSigner for Signer {
    async fn sign(&self, payload: Value) -> BlueprintResult<String> {
        self.0.lock().unwrap().push(payload);
        Ok("owned-token".into())
    }
}
fn app(store: Arc<MemoryAuthStore>) -> AuthHttp<MemoryAuthStore> {
    AuthHttp::new(store, "http://localhost:3000".into(), vec![42; 32], vec![]).unwrap()
}
#[test]
fn date_configuration_requires_millisecond_precision_and_accepts_chrono_date_range() {
    let store = Arc::new(MemoryAuthStore::default());
    for nanos in [1, 999_999, 1_000_001, 999_999_999] {
        assert!(
            app(store.clone())
                .with_jwt_expiration_at(DateTime::from_timestamp(0, nanos).unwrap())
                .is_err()
        );
    }
    // Chrono's supported years already fit within JavaScript's wider Date range.
    for millis in [
        DateTime::<Utc>::MIN_UTC.timestamp_millis(),
        DateTime::<Utc>::MAX_UTC.timestamp_millis(),
        -1,
        0,
        1,
    ] {
        assert!(
            app(store.clone())
                .with_jwt_expiration_at(DateTime::from_timestamp_millis(millis).unwrap())
                .is_ok()
        );
    }
}
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn absolute_dates_floor_negative_milliseconds_and_preserve_payload_subject_and_cookie_rules()
{
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
        for millis in [1_800_000_000_123, 1_800_000_000_999, 0, -1, -1001] {
            for custom in [0, 1, 2] {
                let store = Arc::new(MemoryAuthStore::default());
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(
                    Model::User,
                    json!({"id":"owner","email":"expiration@example.test"}),
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
                let session = SessionService::new(store.clone())
                    .create("owner", "", "", now)
                    .await
                    .unwrap();
                let cookie = SessionCookie { secure: false }.serialize(
                    session["token"].as_str().unwrap(),
                    &[42; 32],
                    true,
                );
                let signed = Arc::new(Mutex::new(Vec::new()));
                let mut service = app(store.clone())
                    .with_clock(move || now)
                    .with_jwt_expiration_at(DateTime::from_timestamp_millis(millis).unwrap())
                    .unwrap()
                    .with_jwt_subject_resolver(|_| async { Ok(Some("resolved-subject".into())) });
                if custom > 0 {
                    service = service.with_jwt_payload_resolver(move |_| async move {
                        Ok(Some(
                            json!({"iat":123,"exp":if custom == 1 { json!(456) } else { Value::Null },"sub":"ignored"})
                                .as_object()
                                .unwrap()
                                .clone(),
                        ))
                    });
                }
                if external {
                    service = service.with_external_jwt_signer(
                        ExternalJwtSigner::new(
                            "https://keys.example.test/public",
                            "EdDSA",
                            Signer(signed.clone()),
                        )
                        .unwrap(),
                    );
                }
                let router = service.router();
                for path in ["/get-session", "/token"] {
                    let response = router
                        .clone()
                        .oneshot(
                            Request::builder()
                                .uri(path)
                                .header("cookie", cookie.split(';').next().unwrap())
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(
                        response.status().as_u16(),
                        200,
                        "{millis}/{custom}/{external}/{path}"
                    );
                    let header = response
                        .headers()
                        .get("set-auth-jwt")
                        .map(|value| value.to_str().unwrap().to_owned());
                    let body: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 65536).await.unwrap(),
                    )
                    .unwrap();
                    let token = if path == "/token" {
                        body["token"].as_str().unwrap().to_owned()
                    } else {
                        header.unwrap()
                    };
                    let claims = if external {
                        signed.lock().unwrap().last().unwrap().clone()
                    } else {
                        serde_json::from_slice::<Value>(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        claims["exp"].as_i64(),
                        Some(if custom == 1 {
                            456
                        } else {
                            millis.div_euclid(1000)
                        })
                    );
                    assert_eq!(claims["sub"], "resolved-subject");
                    if custom > 0 {
                        assert_eq!(claims["iat"], 123);
                        assert!(claims.get("email").is_none());
                    } else {
                        assert_eq!(claims["email"], "expiration@example.test");
                        assert_eq!(claims["sessionId"], session["id"]);
                    }
                    if path == "/get-session" {
                        assert_eq!(body["user"]["id"], "owner");
                        assert_eq!(body["session"]["expiresAt"], session["expiresAt"]);
                    }
                }
            }
        }
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn relative_lifetimes_match_safe_iat_boundaries_and_claim_precedence() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
        for (seconds, iat) in
            [0_u64, 1, 900, 9_007_199_254_740_991]
                .into_iter()
                .flat_map(|seconds| {
                    [123_i64, 9_007_199_254_740_991, -9_007_199_254_740_991]
                        .map(|iat| (seconds, iat))
                })
        {
            for custom in [0, 1, 2] {
                let store = Arc::new(MemoryAuthStore::default());
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(
                    Model::User,
                    json!({"id":"owner","email":"expiration@example.test"}),
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
                let session = SessionService::new(store.clone())
                    .create("owner", "", "", now)
                    .await
                    .unwrap();
                let cookie = SessionCookie { secure: false }.serialize(
                    session["token"].as_str().unwrap(),
                    &[42; 32],
                    true,
                );
                let signed = Arc::new(Mutex::new(Vec::new()));
                let mut service = app(store.clone())
                    .with_clock(move || now)
                    .with_jwt_lifetime(std::time::Duration::from_secs(seconds))
                    .unwrap()
                    .with_jwt_subject_resolver(|_| async { Ok(Some("resolved-subject".into())) });
                {
                    service = service.with_jwt_payload_resolver(move |_| async move {
                        Ok(Some(
                            (if custom == 0 { json!({"iat":iat,"sub":"ignored"}) } else { json!({"iat":iat,"exp":if custom == 1 { json!(456) } else { Value::Null },"sub":"ignored"}) })
                                .as_object()
                                .unwrap()
                                .clone(),
                        ))
                    });
                }
                if external {
                    service = service.with_external_jwt_signer(
                        ExternalJwtSigner::new(
                            "https://keys.example.test/public",
                            "EdDSA",
                            Signer(signed.clone()),
                        )
                        .unwrap(),
                    );
                }
                let router = service.router();
                for path in ["/get-session", "/token"] {
                    let response = router
                        .clone()
                        .oneshot(
                            Request::builder()
                                .uri(path)
                                .header("cookie", cookie.split(';').next().unwrap())
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(
                        response.status().as_u16(),
                        200,
                        "{seconds}/{iat}/{custom}/{external}/{path}"
                    );
                    let header = response
                        .headers()
                        .get("set-auth-jwt")
                        .map(|value| value.to_str().unwrap().to_owned());
                    let body: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 65536).await.unwrap(),
                    )
                    .unwrap();
                    let token = if path == "/token" {
                        body["token"].as_str().unwrap().to_owned()
                    } else {
                        header.unwrap()
                    };
                    let claims = if external {
                        signed.lock().unwrap().last().unwrap().clone()
                    } else {
                        serde_json::from_slice::<Value>(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        claims["exp"].as_f64(),
                        Some(if custom == 1 {
                            456.0
                        } else {
                            json!(iat).as_f64().unwrap() + json!(seconds).as_f64().unwrap()
                        })
                    );
                    assert_eq!(claims["sub"], "resolved-subject");
                    assert_eq!(claims["iat"], iat);
                    assert!(claims.get("email").is_none());
                    if path == "/get-session" {
                        assert_eq!(body["user"]["id"], "owner");
                        assert_eq!(body["session"]["expiresAt"], session["expiresAt"]);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn lifetime_validation_precedes_storage_and_last_expiration_setter_wins() {
    use std::time::Duration;
    let store = Arc::new(MemoryAuthStore::default());
    for value in [
        Duration::from_nanos(1),
        Duration::from_millis(1500),
        Duration::from_secs(9_007_199_254_740_992),
        Duration::MAX,
    ] {
        assert!(app(store.clone()).with_jwt_lifetime(value).is_err());
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    let date = DateTime::from_timestamp_millis(-1).unwrap();
    let relative = app(store.clone())
        .with_jwt_expiration_at(date)
        .unwrap()
        .with_jwt_lifetime(Duration::ZERO)
        .unwrap();
    assert_eq!(
        relative.jwt_expiration.as_ref().unwrap().option(),
        json!("0s")
    );
    let absolute = relative.with_jwt_expiration_at(date).unwrap();
    assert_eq!(
        absolute.jwt_expiration.as_ref().unwrap().option(),
        json!(-1)
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn numeric_seconds_preserve_fractions_payload_subject_and_cookie_rules() {
    let now = DateTime::<Utc>::from_timestamp(1_800_000_000, 0).unwrap();
    for external in [false, true] {
        for seconds in [
            1_800_000_000.125,
            0.0,
            -0.5,
            9_007_199_254_740_992.0,
            f64::MAX,
        ] {
            for custom in [0, 1, 2] {
                let store = Arc::new(MemoryAuthStore::default());
                let mut tx = store.begin(true).await.unwrap();
                tx.insert(
                    Model::User,
                    json!({"id":"owner","email":"expiration@example.test"}),
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
                let session = SessionService::new(store.clone())
                    .create("owner", "", "", now)
                    .await
                    .unwrap();
                let cookie = SessionCookie { secure: false }.serialize(
                    session["token"].as_str().unwrap(),
                    &[42; 32],
                    true,
                );
                let signed = Arc::new(Mutex::new(Vec::new()));
                let mut service = app(store.clone())
                    .with_clock(move || now)
                    .with_jwt_expiration_seconds(seconds)
                    .unwrap()
                    .with_jwt_subject_resolver(|_| async { Ok(Some("resolved-subject".into())) });
                if custom > 0 {
                    service = service.with_jwt_payload_resolver(move |_| async move {
                        Ok(Some(
                            json!({"iat":123,"exp":if custom == 1 { json!(456) } else { Value::Null },"sub":"ignored"})
                                .as_object()
                                .unwrap()
                                .clone(),
                        ))
                    });
                }
                if external {
                    service = service.with_external_jwt_signer(
                        ExternalJwtSigner::new(
                            "https://keys.example.test/public",
                            "EdDSA",
                            Signer(signed.clone()),
                        )
                        .unwrap(),
                    );
                }
                let router = service.router();
                for path in ["/get-session", "/token"] {
                    let response = router
                        .clone()
                        .oneshot(
                            Request::builder()
                                .uri(path)
                                .header("cookie", cookie.split(';').next().unwrap())
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(
                        response.status().as_u16(),
                        200,
                        "{seconds}/{custom}/{external}/{path}"
                    );
                    let header = response
                        .headers()
                        .get("set-auth-jwt")
                        .map(|value| value.to_str().unwrap().to_owned());
                    let body: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 65536).await.unwrap(),
                    )
                    .unwrap();
                    let token = if path == "/token" {
                        body["token"].as_str().unwrap().to_owned()
                    } else {
                        header.unwrap()
                    };
                    let claims = if external {
                        signed.lock().unwrap().last().unwrap().clone()
                    } else {
                        serde_json::from_slice::<Value>(
                            &URL_SAFE_NO_PAD
                                .decode(token.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap()
                    };
                    assert_eq!(
                        claims["exp"].as_f64(),
                        Some(if custom == 1 { 456.0 } else { seconds })
                    );
                    assert_eq!(claims["sub"], "resolved-subject");
                    if custom > 0 {
                        assert_eq!(claims["iat"], 123);
                        assert!(claims.get("email").is_none());
                    } else {
                        assert_eq!(claims["email"], "expiration@example.test");
                        assert_eq!(claims["sessionId"], session["id"]);
                    }
                    if path == "/get-session" {
                        assert_eq!(body["user"]["id"], "owner");
                        assert_eq!(body["session"]["expiresAt"], session["expiresAt"]);
                    }
                }
            }
        }
    }
}

#[tokio::test]
async fn numeric_expiration_rejects_nonfinite_before_storage_and_all_setters_are_last_wins() {
    let store = Arc::new(MemoryAuthStore::default());
    for seconds in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            app(store.clone())
                .with_jwt_expiration_seconds(seconds)
                .is_err()
        );
    }
    let mut tx = store.begin(false).await.unwrap();
    assert!(tx.list(Model::Jwks).await.unwrap().is_empty());
    tx.commit().await.unwrap();
    let numeric = app(store)
        .with_jwt_lifetime(std::time::Duration::ZERO)
        .unwrap()
        .with_jwt_expiration_seconds(-0.5)
        .unwrap();
    assert_eq!(
        numeric.jwt_expiration.as_ref().unwrap().option(),
        json!(-0.5)
    );
    let dated = numeric
        .with_jwt_expiration_at(DateTime::from_timestamp_millis(-1).unwrap())
        .unwrap();
    assert_eq!(dated.jwt_expiration.as_ref().unwrap().option(), json!(-1));
    let numeric = dated.with_jwt_expiration_seconds(1.25).unwrap();
    assert_eq!(
        numeric.jwt_expiration.as_ref().unwrap().option(),
        json!(1.25)
    );
    let lifetime = numeric
        .with_jwt_lifetime(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        lifetime.jwt_expiration.as_ref().unwrap().option(),
        json!("1s")
    );
}
