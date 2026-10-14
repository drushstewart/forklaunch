use super::*;
use forklaunch_auth::oidc::OidcProvider;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    issuer: String,
    response: Arc<Mutex<String>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let metadata = json!({"issuer":issuer,"authorization_endpoint":format!("{issuer}/authorize"),"token_endpoint":format!("{issuer}/token"),"jwks_uri":format!("{issuer}/jwks"),"response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"]}).to_string();
        let keys = json!({"keys":[{"kty":"RSA","alg":"RS256","use":"sig","kid":"fixture","e":"AQAB","n":"kPszNF2ghKj80PvXDNA8pzL2TNEoJKp5g5eIbpPmjHJvyaItOcQOhKcvg5SkW7WVePRqGKIslE-NOg4qMS35oos1g5V9rOnsEunTRciygsCQkaL5WVBNerFLpuUBGypUOYd2W6TpKIsdENXBYO-SDckAYxEZUEdjZshi_AsgWQbfki7axKB67DgTonA0utX3JpG4h9PDVgj6erihubsUdM40Wuslh_j92H1Y3uGQBcvAnwlgvfd_t6WULNeeLAtng-ewemnqEbGSmuZiLPNU2TO9fnwa0C0Cm802BRyYdqDlw3JmLatuqLiO4Ft6PxQDSwlZa1uqy8Y0E3QHhGLp3Q"}]}).to_string();
        let response = Arc::new(Mutex::new(String::new()));
        let reply = response.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    let request = String::from_utf8_lossy(&bytes);
                    if let Some(end) = request.find("\r\n\r\n") {
                        let length = request[..end]
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|value| value.parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                let body = if request.starts_with("GET /.well-known/") {
                    metadata.clone()
                } else if request.starts_with("GET /jwks") {
                    keys.clone()
                } else {
                    reply.lock().unwrap().clone()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            issuer,
            response,
            task,
        }
    }
    fn token(&self, nonce: &str, subject: &str) {
        let now = chrono::Utc::now().timestamp();
        let claims = json!({"iss":self.issuer,"aud":"local-client","sub":subject,"iat":now,"exp":now+300,"nonce":nonce,"email":"one@example.com","email_verified":true,"roles":["admin"]});
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some("fixture".into());
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(include_bytes!(
            "../../../jwt_sessions/test-key.pem"
        ))
        .unwrap();
        let token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
        *self.response.lock().unwrap() =
            json!({"access_token":"provider-secret","token_type":"Bearer","id_token":token})
                .to_string();
    }
}

#[tokio::test]
#[ignore = "requires FORKLAUNCH_TEST_POSTGRES_URL"]
async fn postgres_oidc_exchange_issues_only_explicitly_bound_verified_sessions() {
    let fixture = Fixture::new().await;
    let provider = OidcProvider::discover(
        &fixture.issuer,
        "local-client".into(),
        None,
        "http://127.0.0.1/callback",
    )
    .await
    .unwrap();
    let (owner, pool, role) = super::super::super::tests::test_pools().await;
    let store = PostgresIamService::new(
        pool.clone(),
        TenantContext::new(format!("oidc-{}", uuid::Uuid::new_v4())).unwrap(),
    )
    .await
    .unwrap();
    let store = store.with_jwt_session_issuer(Arc::new(crate::jwt_sessions::tests::issuer()));
    store
        .create_user(User::new("one", "one@example.com"), "password".into())
        .await
        .unwrap();
    for (subject, wrong_nonce, bound) in [
        ("subject", false, false),
        ("subject", true, true),
        ("subject", false, true),
    ] {
        if bound {
            store
                .bind_provider_identity(&fixture.issuer, subject, "one")
                .await
                .unwrap();
        }
        let pending = provider.begin(&[]).unwrap();
        let state = pending.state().to_owned();
        let nonce = pending
            .authorization_url()
            .query_pairs()
            .find(|(name, _)| name == "nonce")
            .unwrap()
            .1
            .into_owned();
        fixture.token(if wrong_nonce { "attacker" } else { &nonce }, subject);
        let result = store
            .authenticate_oidc(&provider, pending, &state, "code")
            .await;
        if bound && !wrong_nonce {
            let session = result.unwrap();
            assert_eq!(session.user_id, "one");
            let claims = crate::jwt_sessions::tests::verify(&session.id);
            assert_eq!(claims["sub"], "one");
            assert_eq!(claims["roles"], json!([]));
            assert!(store.validate_session(&session.id).await.unwrap().is_some());
            assert!(
                store
                    .get_user("one")
                    .await
                    .unwrap()
                    .unwrap()
                    .roles
                    .is_empty()
            );
            store.revoke_session(&session.id).await.unwrap();
        } else {
            assert!(result.is_err());
        }
    }
    store.delete_user("one").await.unwrap();
    pool.close().await;
    sqlx::query(&format!("DROP OWNED BY {role}"))
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query(&format!("DROP ROLE {role}"))
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
}
