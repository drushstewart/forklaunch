//! Trusted optional payload policy; persisted cookie identity remains independent.
use super::{AuthHttp, AuthStore, JwtSessionContext, KeyRing, SessionView};
use chrono::{DateTime, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Map, Value, json};
use std::{future::Future, pin::Pin, sync::Arc};
type PayloadFuture =
    Pin<Box<dyn Future<Output = BlueprintResult<Option<Map<String, Value>>>> + Send>>;
pub(super) struct PayloadResolver(Arc<dyn Fn(JwtSessionContext) -> PayloadFuture + Send + Sync>);
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Replaces the configured session JWT payload with trusted server claims.
    /// `None` means an empty payload, matching nullish JavaScript object spread;
    /// without a callback, the configured session/email/organization claims remain.
    /// The subject policy runs afterward and overrides any payload `sub` field.
    /// Callbacks receive secret-bearing persisted snapshots; never expose them.
    /// Primitive and array object-spread returns are outside this typed interface.
    #[must_use]
    pub fn with_jwt_payload_resolver<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(JwtSessionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<Option<Map<String, Value>>>> + Send + 'static,
    {
        self.payload_resolver = Some(PayloadResolver(Arc::new(move |context| {
            Box::pin(callback(context))
        })));
        self
    }
    pub(super) async fn jwt_with_payload(
        &self,
        view: &SessionView,
        now: DateTime<Utc>,
    ) -> BlueprintResult<String> {
        let failure =
            || BlueprintError::new("HTTP_INTERNAL_EMPTY", "Unable to resolve JWT payload");
        let resolver = self.payload_resolver.as_ref().ok_or_else(failure)?;
        let custom = (resolver.0)(JwtSessionContext::from_view(view))
            .await
            .map_err(|_| failure())?;
        let mut payload = Map::new();
        payload.insert("iat".into(), json!(now.timestamp()));
        payload.extend(custom.unwrap_or_default());
        payload.insert("sub".into(), Value::String(self.jwt_subject(view).await?));
        let body = json!({"payload": payload});
        let ring = KeyRing::new(self.store.clone(), self.auth_origin());
        let signed = if let Some(signer) = &self.external_jwt {
            ring.sign_jwt_with_signer(Some(&body), now, signer).await
        } else {
            ring.sign_jwt(Some(&body), now).await
        }
        .map_err(|_| failure())?;
        super::text(&signed, "token").map(str::to_owned)
    }
}
#[cfg(test)]
mod tests;
