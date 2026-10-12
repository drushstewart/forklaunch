use super::*;
use crate::{
    keyring::KeyRing,
    store::{AuthTransaction, Model, memory::MemoryAuthStore},
};
use chrono::{SecondsFormat, Utc};
use forklaunch_auth::{
    jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation},
    request_hmac::{SignedBody, generate_hmac_auth_header},
};
const SECRET: &[u8] = b"application-http-test-secret-32-characters";
const USER: &str = "11000000-0000-4000-8000-000000000001";

fn signed(path: &str) -> String {
    generate_hmac_auth_header(
        SECRET,
        "default",
        "GET",
        path,
        SignedBody::Absent,
        &Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        "application-fixture",
    )
    .unwrap()
}
async fn rpc(client: &reqwest::Client, base: &str, method: &str, params: Value) -> Value {
    let response = client
        .post(format!("{base}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.json().await.unwrap()
}

#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn application_http_mcp_auth_roles_compliance_and_shutdown() {
    let now = Utc::now();
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    for row in [
        json!({"id":USER,"email":"system@fixture.test"}),
        json!({"id":"outsider","email":"outsider@fixture.test"}),
    ] {
        tx.insert(Model::User, row).await.unwrap();
    }
    tx.insert(Model::Session, json!({"id":"active","userId":USER,"activeOrganizationId":"org","createdAt":now.to_rfc3339(),"expiresAt":(now+chrono::Duration::hours(1)).to_rfc3339()})).await.unwrap();
    tx.insert(
        Model::Member,
        json!({"id":"member","userId":USER,"organizationId":"org","role":"system"}),
    )
    .await
    .unwrap();
    tx.insert(Model::OrganizationRole, json!({"id":"permission","organizationId":"org","role":"system","permission":"{\"account\":[\"read\"]}"})).await.unwrap();
    tx.insert(Model::Account, json!({"id":"credential","userId":"subject","password":"private-password","accessToken":null})).await.unwrap();
    tx.commit().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let ring = KeyRing::new(store.clone(), &base);
    let token = ring
        .issue(USER, "system@fixture.test", Some("org"), now)
        .await
        .unwrap();
    let outsider = ring
        .issue("outsider", "outsider@fixture.test", None, now)
        .await
        .unwrap();
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.set_issuer(&[base.as_str()]);
    validation.set_audience(&[base.as_str()]);
    let policy = HmacJwksAuth::new(
        SECRET.to_vec(),
        RemoteJwks::new(RemoteJwksConfig::new(format!("{base}/api/auth/jwks"))).unwrap(),
        validation,
    )
    .unwrap();
    let app = AuthHttp::new(store.clone(), base.clone(), SECRET.to_vec(), vec![])
        .unwrap()
        .application_router(policy, ServiceInfo::new("better-auth-application", "1.0"))
        .into_router_with_default_mcp()
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let discovery = client
        .get(format!("{base}/discovery/auth-methods"))
        .send()
        .await
        .unwrap();
    assert_eq!(discovery.status(), 200);
    assert_eq!(discovery.json::<Value>().await.unwrap(), super::discovery());
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let popup = browser
        .get(format!("{base}/api/auth/test-callback?callbackUrl=%2Fdone"))
        .send()
        .await
        .unwrap();
    assert_eq!(popup.status(), 302);
    assert_eq!(
        popup.headers()["location"],
        format!("{base}/done#token=&sessionToken=&email=unknown")
    );
    assert_eq!(
        popup.headers()["set-cookie"],
        "better-auth.session_token=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"
    );
    let page = client
        .get(format!(
            "{base}/api/auth/oauth-redirect?callbackURL=%2Fdone&provider=google"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert!(
        page.text()
            .await
            .unwrap()
            .contains("credentials: 'include'")
    );
    let init = rpc(&client, &base, "initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}})).await;
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        "better-auth-application"
    );
    let tools = rpc(&client, &base, "tools/list", json!({})).await;
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    let contracts = registry("better-auth-application", "1.0");
    for route in contracts.routes() {
        assert!(
            tools
                .iter()
                .any(|tool| tool["name"] == route.operation_name),
            "{}",
            route.operation_name
        );
    }
    let discovery = rpc(
        &client,
        &base,
        "tools/call",
        json!({"name":"getauthmethods","arguments":{}}),
    )
    .await;
    assert_eq!(
        discovery["result"]["structuredContent"],
        super::discovery(),
        "{discovery}"
    );
    for (suffix, expected) in [
        ("surface-roles", json!([{"name":"system"}])),
        (
            "surface-permissions",
            json!([{"slug":"{\"account\":[\"read\"]}"}]),
        ),
    ] {
        let path = format!("/user/{USER}/{suffix}");
        assert_eq!(
            client
                .get(format!("{base}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let header = signed(&path);
        let response = client
            .get(format!("{base}{path}"))
            .header("authorization", &header)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().await.unwrap(), expected);
        let name = &contracts
            .routes()
            .iter()
            .find(|route| route.path.ends_with(suffix))
            .unwrap()
            .operation_name;
        let call = rpc(&client, &base, "tools/call", json!({"name":name,"arguments":{"params":{"id":USER},"headers":{"authorization":header}}})).await;
        assert_eq!(call["result"]["isError"], false, "{call}");
        assert_eq!(
            serde_json::from_str::<Value>(call["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap(),
            expected
        );
        // Configured IdSchema accepts any string, including non-UUID IDs.
        let invalid = format!("/user/not-a-uuid/{suffix}");
        assert_eq!(
            client
                .get(format!("{base}{invalid}"))
                .header("authorization", signed(&invalid))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        let replay = client
            .get(format!("{base}{path}"))
            .header("authorization", signed("/other-path"))
            .send()
            .await
            .unwrap();
        assert_eq!(replay.status(), 401);
    }
    let export = format!("{base}/compliance/export/subject");
    assert_eq!(client.get(&export).send().await.unwrap().status(), 401);
    assert_eq!(
        client
            .get(&export)
            .bearer_auth(&outsider)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let response = client
        .get(&export)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let exported = response.json::<Value>().await.unwrap();
    assert_eq!(exported["entities"]["Account"][0]["password"], json!({}));
    assert!(!exported.to_string().contains("private-password"));
    let call = rpc(&client, &base, "tools/call", json!({"name":"exportuserdata","arguments":{"params":{"userId":"subject"},"headers":{"authorization":format!("Bearer {token}")}}})).await;
    assert_eq!(call["result"]["structuredContent"], exported);
    // Roles come from the current trusted store, not the JWT or its org claim.
    let mut tx = store.begin(true).await.unwrap();
    tx.replace(
        Model::Member,
        json!({"id":"member","userId":USER,"organizationId":"org","role":"member"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        client
            .get(&export)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut tx = store.begin(true).await.unwrap();
    tx.replace(
        Model::Member,
        json!({"id":"member","userId":USER,"organizationId":"org","role":"system"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let erased = rpc(&client, &base, "tools/call", json!({"name":"eraseuserdata","arguments":{"params":{"userId":"subject"},"headers":{"authorization":format!("Bearer {token}")}}})).await;
    assert_eq!(
        erased["result"]["structuredContent"],
        json!({"entitiesAffected":["Account"],"recordsDeleted":1,"failures":[]})
    );
    assert_eq!(
        client
            .get(&export)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .delete(format!("{base}/compliance/erase/subject"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .get(format!("{base}/api/auth/ok"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let openapi = client
        .get(format!("{base}{}", RouterConfig::default().openapi_path))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(openapi["paths"].as_object().unwrap().len(), 5);
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        client
            .get(format!("{base}/discovery/auth-methods"))
            .send()
            .await
            .is_err()
    );
}
