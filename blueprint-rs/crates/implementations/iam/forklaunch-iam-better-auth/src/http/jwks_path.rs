//! Bounded static JWKS routing; validation precedes Axum route construction.
use super::{AuthHttp, AuthStore};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};

impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Relocates the local JWKS route and its `OpenAPI` operation.
    /// # Errors
    /// Requires a fixed unencoded absolute path outside owned auth namespaces.
    /// Root JWKS is unsupported because nested Axum root URLs differ.
    pub fn with_jwks_path(mut self, path: impl Into<String>) -> BlueprintResult<Self> {
        let path = path.into();
        if path == "/"
            || !path.starts_with('/')
            || path.contains("..")
            || path.contains("//")
            || (path.len() > 1 && path.ends_with('/'))
            || path.split('/').any(|part| part == ".")
            || !path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte))
        {
            return Err(invalid());
        }
        if path != "/jwks" && path != "/" {
            let schema = super::public::openapi_schema("http://auth.invalid", "/jwks");
            let prefix = path.split('/').nth(1).ok_or_else(invalid)?;
            if schema["paths"]
                .as_object()
                .ok_or_else(invalid)?
                .keys()
                .map(String::as_str)
                .chain([
                    "/open-api",
                    "/error",
                    "/reference",
                    "/test-callback",
                    "/oauth-redirect",
                    "/relay",
                    "/user",
                    "/compliance",
                    "/discovery",
                ])
                .any(|owned| owned.split('/').nth(1) == Some(prefix))
            {
                return Err(invalid());
            }
        }
        self.jwks_path = path;
        Ok(self)
    }
    /// Relative JWKS route, for configuring clients and verification URLs.
    #[must_use]
    pub fn jwks_path(&self) -> &str {
        &self.jwks_path
    }
}

fn invalid() -> BlueprintError {
    BlueprintError::new(
        "better_auth_invalid_config",
        "JWKS path must be a nonconflicting fixed path; root JWKS is unsupported",
    )
}

#[cfg(test)]
mod tests;
