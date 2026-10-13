//! Managed-app relay handoffs. The HTTP adapter must authenticate the raw
//! ingest request before calling this trusted service. Nonces commit before
//! application hooks, and tickets commit their one-time claim before minting.
use crate::{
    cookies::SessionCookie,
    store::{AuthStore, AuthTransaction, Model},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{future::Future, sync::Arc};
use zeroize::Zeroizing;

/// Provider token fields accepted by the configured TS relay schema. Unknown
/// JSON fields are ignored. Tokens are never written into the handoff record.
#[derive(Deserialize, Serialize, Default)]
pub struct RelayTokens {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}
#[derive(Default)]
pub struct RelayEstablishment {
    pub redirect_path: String,
    pub owner_user_id: Option<String>,
    pub active_organization_id: Option<String>,
}
/// Implement the application's token persistence and owner-resolution policy.
/// A nonce is already committed when this hook runs, including if it fails.
pub struct AppClaimLink {
    pub claim_url: String,
    pub expires_at: Option<DateTime<Utc>>,
}

pub trait RelayHooks<S: AuthStore>: Send + Sync {
    fn establish(
        &self,
        tokens: RelayTokens,
        store: Arc<S>,
    ) -> impl Future<Output = BlueprintResult<RelayEstablishment>> + Send;

    /// Optional product-specific claim ceremony. The safe scaffold declines.
    fn mint_claim(
        &self,
        _instance_id: &str,
        _store: Arc<S>,
    ) -> impl Future<Output = BlueprintResult<Option<AppClaimLink>>> + Send {
        async { Ok(None) }
    }
}
/// Safe scaffold default: redirect to the instance root without signing in.
pub struct DefaultRelayHooks;
impl<S: AuthStore> RelayHooks<S> for DefaultRelayHooks {
    async fn establish(
        &self,
        _tokens: RelayTokens,
        _store: Arc<S>,
    ) -> BlueprintResult<RelayEstablishment> {
        Ok(RelayEstablishment::default())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayIngestResult {
    pub redirect_path: String,
}
pub struct RelayRedemption {
    pub cookie: Option<String>,
    pub redirect_to: String,
}
pub struct RelayService<S, H> {
    store: Arc<S>,
    hooks: Arc<H>,
    cookie: SessionCookie,
    secret: Zeroizing<Vec<u8>>,
}
impl<S: AuthStore, H: RelayHooks<S>> RelayService<S, H> {
    #[must_use]
    pub fn new(store: Arc<S>, hooks: Arc<H>, cookie: SessionCookie, secret: Vec<u8>) -> Self {
        Self {
            store,
            hooks,
            cookie,
            secret: Zeroizing::new(secret),
        }
    }
    /// # Errors
    /// Rejects reused nonces; storage/hook failures leave the nonce claimed once
    /// its initial insert commits. This matches the generated TypeScript hook.
    pub async fn ingest(
        &self,
        nonce: &str,
        tokens: RelayTokens,
        now: DateTime<Utc>,
    ) -> BlueprintResult<RelayIngestResult> {
        let id = uuid::Uuid::new_v4().to_string();
        let expires = now
            .checked_add_signed(Duration::minutes(5))
            .ok_or_else(invalid)?;
        let mut tx = self.store.begin(true).await?;
        let row = json!({"id":id,"nonce":nonce,"ownerUserId":null,"activeOrganizationId":null,
            "redirectTo":"/","expiresAt":stamp(expires),"consumedAt":null,"createdAt":stamp(now),"updatedAt":stamp(now)});
        if let Err(error) = tx.insert(Model::RelaySessionHandoff, row).await {
            return Err(if error.code == "better_auth_store_conflict" {
                replay()
            } else {
                error
            });
        }
        tx.commit().await?;
        let establishment = self.hooks.establish(tokens, self.store.clone()).await?;
        let mut tx = self.store.begin(true).await?;
        let mut row = tx
            .get(Model::RelaySessionHandoff, &id)
            .await?
            .ok_or_else(invalid)?;
        row["ownerUserId"] = json!(establishment.owner_user_id);
        row["activeOrganizationId"] = json!(establishment.active_organization_id);
        row["redirectTo"] = json!(sanitize_redirect_path(Some(&establishment.redirect_path)));
        tx.replace(Model::RelaySessionHandoff, row).await?;
        tx.commit().await?;
        Ok(RelayIngestResult {
            redirect_path: format!("/relay/handoff?ticket={id}"),
        })
    }
    /// # Errors
    /// Returns storage/key failures. A consumed ticket is not restored if
    /// subsequent session creation fails; browser adapters redirect to `/`.
    pub async fn redeem_ticket(
        &self,
        id: &str,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Option<RelayRedemption>> {
        let mut tx = self.store.begin(true).await?;
        let Some(mut row) = tx.get(Model::RelaySessionHandoff, id).await? else {
            tx.commit().await?;
            return Ok(None);
        };
        let expires = row["expiresAt"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .ok_or_else(invalid)?;
        if !row["consumedAt"].is_null() || expires <= now {
            tx.commit().await?;
            return Ok(None);
        }
        row["consumedAt"] = json!(stamp(now));
        tx.replace(Model::RelaySessionHandoff, row.clone()).await?;
        tx.commit().await?;
        let cookie = if let Some(owner) = row["ownerUserId"]
            .as_str()
            .filter(|owner| !owner.is_empty())
        {
            Some(
                self.mint_cookie(owner, row["activeOrganizationId"].as_str(), now)
                    .await?,
            )
        } else {
            None
        };
        Ok(Some(RelayRedemption {
            cookie,
            redirect_to: sanitize_redirect_path(row["redirectTo"].as_str()).into(),
        }))
    }
    /// # Errors
    /// Rejects a supplied purpose other than app-claim-mint before the hook runs.
    pub async fn mint_claim(
        &self,
        purpose: Option<&str>,
        instance_id: &str,
    ) -> BlueprintResult<Option<AppClaimLink>> {
        if purpose.is_some_and(|purpose| purpose != "app-claim-mint") {
            return Err(BlueprintError::new(
                "relay_claim_purpose",
                "This signature was not made for minting a claim",
            ));
        }
        self.hooks.mint_claim(instance_id, self.store.clone()).await
    }

    async fn mint_cookie(
        &self,
        owner: &str,
        organization: Option<&str>,
        now: DateTime<Utc>,
    ) -> BlueprintResult<String> {
        let mut bytes = [0; 32];
        OsRng.try_fill_bytes(&mut bytes).map_err(|_| invalid())?;
        let token = URL_SAFE_NO_PAD.encode(bytes);
        let expires = now
            .checked_add_signed(Duration::days(1))
            .ok_or_else(invalid)?;
        let mut tx = self.store.begin(true).await?;
        if tx.get(Model::User, owner).await?.is_none() {
            return Err(invalid());
        }
        tx.insert(Model::Session,json!({"id":uuid::Uuid::new_v4().to_string(),"userId":owner,"token":token,
            "createdAt":stamp(now),"updatedAt":stamp(now),"expiresAt":stamp(expires),"ipAddress":null,"userAgent":null,
            "activeOrganizationId":organization,"activeTeamId":null})).await?;
        tx.commit().await?;
        Ok(self.cookie.serialize(&token, &self.secret, true))
    }
}
#[must_use]
pub fn sanitize_redirect_path(candidate: Option<&str>) -> &str {
    candidate
        .filter(|value| {
            !value.bytes().any(|byte| byte.is_ascii_control())
                && value.starts_with('/')
                && !value.starts_with("//")
                && !value.starts_with("/\\")
        })
        .unwrap_or("/")
}
fn stamp(now: DateTime<Utc>) -> String {
    now.to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn invalid() -> BlueprintError {
    BlueprintError::new("relay_invalid", "Unable to complete relay handoff")
}
fn replay() -> BlueprintError {
    BlueprintError::new("relay_replay", "relay nonce already consumed")
}
#[cfg(test)]
pub(crate) mod tests;
