use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, header},
};
use tower::ServiceExt;
async fn call(
    router: axum::Router,
    path: &str,
    method: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, "http://localhost:3000");
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    let data = if let Some(body) = body {
        req = req.header(header::CONTENT_TYPE, "application/json");
        body.to_string()
    } else {
        String::new()
    };
    let out = router
        .oneshot(req.body(Body::from(data)).unwrap())
        .await
        .unwrap();
    let status = out.status().as_u16();
    let data = to_bytes(out.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&data).unwrap_or(Value::Null))
}
#[allow(clippy::too_many_lines)]
pub(super) async fn exercise<S: AuthStore + 'static>(
    store: Arc<S>,
    f: &Fixture,
    features: OrganizationFeatures,
) {
    let now = f.now;
    let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = called.clone();
    let teams = features.teams;
    let app = crate::http::AuthHttp::new(
        store.clone(),
        "http://localhost:3000".into(),
        vec![7; 32],
        vec![],
    )
    .unwrap()
    .with_rate_limiting(false)
    .with_clock(move || now)
    .with_organization_features(features)
    .with_access_control(control())
    .with_jwt_payload_resolver(move |ctx| {
        seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(ctx.session().get("activeTeamId").is_some(), teams);
        async { Ok(Some(serde_json::Map::new())) }
    });
    let router = app.router();
    for path in crate::organizations::features::TEAM_ROUTES
        .iter()
        .chain(crate::organizations::features::ROLE_ROUTES)
    {
        for method in ["GET", "POST", "HEAD"] {
            let (status, _) = call(
                router.clone(),
                path,
                method,
                None,
                (method == "POST").then(|| json!({})),
            )
            .await;
            if !features.route(path) {
                assert_eq!(status, 404, "{path}/{method}");
            } else if method == "HEAD" {
                assert_eq!(status, 404);
            }
        }
    }
    let (status, schema) = call(
        router.clone(),
        "/open-api/generate-schema",
        "GET",
        None,
        None,
    )
    .await;
    assert_eq!(status, 200);
    for path in crate::organizations::features::TEAM_ROUTES
        .iter()
        .chain(crate::organizations::features::ROLE_ROUTES)
    {
        assert_eq!(
            schema["paths"].get(*path).is_some(),
            features.route(path),
            "{path}"
        );
    }
    assert_eq!(
        schema["components"]["schemas"].get("Team").is_some(),
        features.teams
    );
    assert_eq!(
        schema["components"]["schemas"]
            .get("OrganizationRole")
            .is_some(),
        features.dynamic_access_control
    );
    let cookie =
        (crate::cookies::SessionCookie { secure: false }).serialize(&f.owner_token, &[7; 32], true);
    let cookie = cookie.split(';').next().unwrap();
    let (status, session) = call(router.clone(), "/get-session", "GET", Some(cookie), None).await;
    assert_eq!(status, 200);
    assert_eq!(
        session["session"].get("activeTeamId").is_some(),
        features.teams
    );
    assert_eq!(called.load(std::sync::atomic::Ordering::SeqCst), 1);
    let (status, sessions) = call(router, "/list-sessions", "GET", Some(cookie), None).await;
    assert_eq!(status, 200);
    assert!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s.get("activeTeamId").is_some() == features.teams)
    );
    let mut tx = store.begin(false).await.unwrap();
    assert!(
        tx.find_unique(Model::Session, "token", &f.owner_token)
            .await
            .unwrap()
            .unwrap()
            .get("activeTeamId")
            .is_some()
    );
    tx.commit().await.unwrap();
}
