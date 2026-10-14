//! Trusted asynchronous JWT subject policy, separate from cookie identity.
use super::{AuthHttp, AuthStore, SessionView};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use std::{future::Future, pin::Pin, sync::Arc};

/// An owned snapshot of a verified persisted user and session.
/// This is a trusted server callback input, not proof accepted from a caller.
/// Session fields can include the secret session token. Do not log or expose
/// these records. Debug output deliberately excludes their values.
#[derive(Clone)]
pub struct JwtSessionContext {
    user: Value,
    session: Value,
}
impl JwtSessionContext {
    /// Persisted user fields; the callback cannot mutate the authoritative row.
    #[must_use]
    pub fn user(&self) -> &Value {
        &self.user
    }
    /// Persisted session fields, potentially including a secret session token.
    #[must_use]
    pub fn session(&self) -> &Value {
        &self.session
    }
}
impl std::fmt::Debug for JwtSessionContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("JwtSessionContext")
            .finish_non_exhaustive()
    }
}
type SubjectFuture = Pin<Box<dyn Future<Output = BlueprintResult<Option<String>>> + Send>>;
pub(super) struct SubjectResolver(Arc<dyn Fn(JwtSessionContext) -> SubjectFuture + Send + Sync>);
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Installs the optional trusted `jwt.getSubject` policy. `None` preserves
    /// the user ID; an empty subject is preserved. Cookie identity is unchanged.
    /// Custom subjects may require separate application authorization mapping.
    #[must_use]
    pub fn with_jwt_subject_resolver<F, Fut>(mut self, callback: F) -> Self
    where
        F: Fn(JwtSessionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<Option<String>>> + Send + 'static,
    {
        self.subject_resolver = Some(SubjectResolver(Arc::new(move |context| {
            Box::pin(callback(context))
        })));
        self
    }
    pub(super) async fn jwt_subject(&self, view: &SessionView) -> BlueprintResult<String> {
        if let Some(resolver) = &self.subject_resolver {
            let context = JwtSessionContext {
                user: view.user.clone(),
                session: view.session.clone(),
            };
            let subject = (resolver.0)(context).await.map_err(|_| {
                BlueprintError::new("HTTP_INTERNAL_EMPTY", "Unable to resolve JWT subject")
            })?;
            if let Some(subject) = subject {
                return Ok(subject);
            }
        }
        super::text(&view.user, "id").map(str::to_owned)
    }
}
#[cfg(test)]
mod tests;
