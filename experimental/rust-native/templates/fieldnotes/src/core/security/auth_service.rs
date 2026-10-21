//! Composition of the trusted session transport and application identity policy.
use crate::{Role, SecurityError, VerifiedTenant};
use forklaunch_core::auth::session_client::{SessionClient, SessionClientError, SessionTransport};
use serde::Deserialize;
use uuid::Uuid;

/// Application auth client: validates workspace claims and applies the app role model.
pub struct SecurityService {
    client: SessionClient,
    internal_secret: zeroize::Zeroizing<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthReply {
    user_id: String,
    workspace_id: Uuid,
    role: Role,
}
fn auth_error(error: SessionClientError) -> SecurityError {
    match error {
        SessionClientError::Unauthorized => SecurityError::Unauthorized,
        SessionClientError::Forbidden => SecurityError::Forbidden,
        SessionClientError::InvalidConfiguration => SecurityError::InvalidInput,
        SessionClientError::Unavailable => SecurityError::Unavailable,
    }
}
impl SecurityService {
    pub fn new(auth_base_url: String, internal_secret: String) -> Result<Self, SecurityError> {
        let transport = if std::env::var("ALLOW_PLAINTEXT_AUTH_TRANSPORT").as_deref() == Ok("true")
        {
            SessionTransport::PlaintextDevelopment
        } else {
            SessionTransport::TlsOrLoopback
        };
        let client = SessionClient::with_transport(
            &format!("{}/internal/session", auth_base_url.trim_end_matches('/')),
            "x-fieldnotes-internal",
            internal_secret.clone(),
            transport,
        )
        .map_err(auth_error)?;
        Ok(Self {
            client,
            internal_secret: zeroize::Zeroizing::new(internal_secret),
        })
    }
    pub fn authenticate_internal(&self, supplied: &str) -> bool {
        self.client.authenticate_internal(supplied)
    }
    pub async fn authenticate_signed_internal<F, Fut, E>(
        &self,
        header: &str,
        path: &str,
        body: &[u8],
        claim: F,
    ) -> Result<(), SecurityError>
    where
        F: FnOnce(String, i64) -> Fut,
        Fut: std::future::Future<Output = Result<bool, E>>,
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| SecurityError::Unauthorized)?
            .as_millis();
        forklaunch_core::auth::request_hmac::verify_with_nonce_store(
            header,
            self.internal_secret.as_bytes(),
            "worker",
            "POST",
            path,
            forklaunch_core::auth::request_hmac::SignedBody::Bytes(body),
            i64::try_from(now).map_err(|_| SecurityError::Unauthorized)?,
            std::time::Duration::from_secs(60),
            claim,
        )
        .await
        .map_err(|_| SecurityError::Unauthorized)
    }
    pub async fn authenticate(
        &self,
        cookie: &str,
        workspace_id: Uuid,
    ) -> Result<VerifiedTenant, SecurityError> {
        let reply: AuthReply = self
            .client
            .verify(cookie, &serde_json::json!({"workspaceId":workspace_id}))
            .await
            .map_err(auth_error)?
            .into_inner();
        if reply.workspace_id != workspace_id
            || reply.user_id.is_empty()
            || reply.user_id.len() > 256
        {
            return Err(SecurityError::Unavailable);
        }
        Ok(VerifiedTenant::from_verified_parts(
            reply.user_id,
            reply.workspace_id,
            reply.role,
        ))
    }
}
