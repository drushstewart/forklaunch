use super::*;
use crate::{
    relay::{AppClaimLink, RelayEstablishment},
    store::{AuthTransaction, Model, memory::MemoryAuthStore},
};
use chrono::Utc;
use forklaunch_auth::{
    jwt::{Algorithm, RemoteJwks, RemoteJwksConfig, Validation},
    request_hmac::{SignedBody, generate_hmac_auth_header},
};
use std::sync::atomic::{AtomicUsize, Ordering};
const DEFAULT: &[u8] = b"relay-default-fixture-secret-32-characters";
const INSTANCE: &[u8] = b"relay-instance-fixture-secret";
struct Hooks {
    claims: AtomicUsize,
}
impl RelayHooks<MemoryAuthStore> for Hooks {
    async fn establish(
        &self,
        tokens: RelayTokens,
        _store: Arc<MemoryAuthStore>,
    ) -> forklaunch_blueprint_core::BlueprintResult<RelayEstablishment> {
        assert_eq!(tokens.access_token.as_deref(), Some("fixture-token"));
        Ok(RelayEstablishment {
            redirect_path: "/done café".into(),
            owner_user_id: Some("relay-http-user".into()),
            active_organization_id: Some("org".into()),
        })
    }
    async fn mint_claim(
        &self,
        instance: &str,
        _store: Arc<MemoryAuthStore>,
    ) -> forklaunch_blueprint_core::BlueprintResult<Option<AppClaimLink>> {
        self.claims.fetch_add(1, Ordering::SeqCst);
        Ok((instance == "instance-1").then(|| AppClaimLink {
            claim_url: "https://frontend.fixture/claim/owned".into(),
            expires_at: None,
        }))
    }
}
fn sign(path: &str, body: &str, id: &str, secret: &[u8]) -> String {
    generate_hmac_auth_header(
        secret,
        id,
        "POST",
        path,
        SignedBody::Text(body),
        &Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "fixture-signature-nonce",
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
async fn post(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &str,
    id: &str,
    secret: &[u8],
) -> reqwest::Response {
    client
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .header("authorization", sign(path, body, id, secret))
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn live_http_mcp_route_keys_claim_purpose_browser_cookie_and_shutdown() {
    let store = Arc::new(MemoryAuthStore::default());
    let mut tx = store.begin(true).await.unwrap();
    tx.insert(
        Model::User,
        json!({"id":"relay-http-user","email":"relay-http@fixture.test"}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hooks = Arc::new(Hooks {
        claims: AtomicUsize::new(0),
    });
    let auth = HmacJwksAuth::new(
        DEFAULT.to_vec(),
        RemoteJwks::new(RemoteJwksConfig::new(format!("{base}/api/auth/jwks"))).unwrap(),
        Validation::new(Algorithm::EdDSA),
    )
    .unwrap();
    let auth = configure_auth(auth, DEFAULT, "instance-1", INSTANCE).unwrap();
    let router = AuthHttp::new(store, base.clone(), DEFAULT.to_vec(), vec![])
        .unwrap()
        .application_router_with_relay(auth, ServiceInfo::new("relay-fixture", "1"), hooks.clone())
        .into_router_with_default_mcp()
        .unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
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
    let path = "/relay/session-ingest";
    let body =
        r#"{"nonce":"relay-live","tokens":{"access_token":"fixture-token","ignored":"drop"}}"#;
    for (id, key) in [
        ("default", DEFAULT),
        ("platform", INSTANCE),
        ("instance-1", DEFAULT),
    ] {
        assert_eq!(
            post(&client, &base, path, body, id, key).await.status(),
            401
        );
    }
    let ingested = post(&client, &base, path, body, "instance-1", INSTANCE).await;
    assert_eq!(ingested.status(), 200);
    let result: Value = ingested.json().await.unwrap();
    let browser = client
        .get(format!(
            "{base}{}",
            result["redirectPath"].as_str().unwrap()
        ))
        .header("accept", "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(browser.status(), 302);
    assert_eq!(browser.headers()["location"], "/done%20caf%C3%A9");
    assert_eq!(browser.headers()["vary"], "Accept");
    let cookie = browser.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert!(
        browser
            .text()
            .await
            .unwrap()
            .contains("Found. Redirecting to")
    );
    let session = client
        .get(format!("{base}/api/auth/get-session"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(session["user"]["id"], "relay-http-user");
    let replay = client
        .get(format!(
            "{base}{}",
            result["redirectPath"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.headers()["location"], "/");
    assert!(!replay.headers().contains_key("set-cookie"));
    assert_eq!(
        post(&client, &base, path, body, "instance-1", INSTANCE)
            .await
            .status(),
        409
    );
    assert_eq!(
        post(
            &client,
            &base,
            "/claim/mint",
            r#"{"purpose":"wrong"}"#,
            "platform",
            INSTANCE
        )
        .await
        .status(),
        400
    );
    assert_eq!(hooks.claims.load(Ordering::SeqCst), 0);
    let minted = post(
        &client,
        &base,
        "/claim/mint",
        r#"{"purpose":"app-claim-mint","instanceId":"instance-1"}"#,
        "platform",
        INSTANCE,
    )
    .await;
    assert_eq!(minted.status(), 200);
    assert_eq!(
        minted.json::<Value>().await.unwrap()["claimUrl"],
        "https://frontend.fixture/claim/owned"
    );
    assert_eq!(
        post(&client, &base, "/claim/mint", "{}", "default", DEFAULT)
            .await
            .status(),
        409
    );
    assert_eq!(
        post(&client, &base, "/claim/mint", "{}", "instance-1", INSTANCE)
            .await
            .status(),
        401
    );
    let init = rpc(&client,&base,"initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}})).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "relay-fixture");
    let tools = rpc(&client, &base, "tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 9);
    let mcp_body = json!({"nonce":"relay-mcp","tokens":{"access_token":"fixture-token"}});
    let signed = sign(path, &mcp_body.to_string(), "instance-1", INSTANCE);
    let invoked = rpc(&client,&base,"tools/call",json!({"name":contracts()[0].operation_name,"arguments":{"body":mcp_body,"headers":{"authorization":signed}}})).await;
    assert!(
        invoked["result"]["structuredContent"]["redirectPath"]
            .as_str()
            .unwrap()
            .starts_with("/relay/handoff?ticket="),
        "{invoked}"
    );
    stop.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        client
            .get(format!("{base}/relay/handoff"))
            .send()
            .await
            .is_err()
    );
}
#[test]
fn redirect_encoding_matches_express_encodeurl() {
    for (input, expected) in [
        ("/100%", "/100%25"),
        ("/%A", "/%A"),
        ("/%G", "/%25G"),
        ("/%[", "/%25%5B"),
        ("/é path", "/%C3%A9%20path"),
        ("/[]{}\\bar", "/[]%7B%7D\\bar"),
    ] {
        assert_eq!(encode_location(input), expected);
    }
}
