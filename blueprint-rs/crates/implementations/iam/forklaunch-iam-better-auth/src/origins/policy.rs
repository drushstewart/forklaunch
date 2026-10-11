use super::{matches_origin, origin};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use url::Url;

pub struct OriginPolicy {
    trusted: Vec<String>,
}
/// Header presence matters: even an empty Cookie header triggers CSRF checks.
#[derive(Default)]
pub struct RequestOrigins<'a> {
    pub cookie_present: bool,
    pub origin: Option<&'a str>,
    pub referer: Option<&'a str>,
    pub fetch_site: Option<&'a str>,
    pub fetch_mode: Option<&'a str>,
    pub fetch_dest: Option<&'a str>,
    /// Actual request origin, without trusting forwarded headers. Used only for
    /// Origin:null combined with Sec-Fetch-Site:same-origin.
    pub request_origin: &'a str,
}
impl OriginPolicy {
    /// Configures static trusted origins, including the configured base origin.
    /// # Errors
    /// Rejects invalid/non-HTTP base URLs and embedded credentials.
    pub fn new(base_url: &str, mut trusted: Vec<String>) -> BlueprintResult<Self> {
        let url = Url::parse(base_url).map_err(|_| invalid_config())?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.host().is_none()
        {
            return Err(invalid_config());
        }
        trusted.push(origin(&url).ok_or_else(invalid_config)?);
        Ok(Self { trusted })
    }
    #[must_use]
    pub fn trusted(&self, value: &str, allow_relative: bool) -> bool {
        self.trusted
            .iter()
            .any(|pattern| matches_origin(value, pattern, allow_relative))
    }
    /// Applies configured mutation Origin/callback checks and optional login
    /// Fetch Metadata protection. GET/HEAD/OPTIONS bypass this mutation guard;
    /// endpoint-specific GET callback validation must be applied separately.
    /// # Errors
    /// Returns actual Origin/CSRF/callback denial codes.
    pub fn mutation(
        &self,
        request: &RequestOrigins<'_>,
        login: bool,
        body: &Value,
    ) -> BlueprintResult<()> {
        self.validate_origin(request, false)?;
        for (field, code) in [
            ("callbackURL", "INVALID_CALLBACK_URL"),
            ("redirectTo", "INVALID_REDIRECT_URL"),
            ("errorCallbackURL", "INVALID_ERROR_CALLBACK_URL"),
            ("newUserCallbackURL", "INVALID_NEW_USER_CALLBACK_URL"),
        ] {
            if let Some(value) = body.get(field).filter(|value| truthy(value)) {
                let value = value.as_str().ok_or_else(|| {
                    BlueprintError::new(
                        "INVALID_CALLBACK_VALUE_TYPE",
                        format!(
                            "Invalid {}: expected a string",
                            if field == "redirectTo" {
                                "redirectURL"
                            } else {
                                field
                            }
                        ),
                    )
                })?;
                if !self.trusted(value, true) {
                    return Err(BlueprintError::new(
                        code,
                        format!(
                            "Invalid {}",
                            if field == "redirectTo" {
                                "redirectURL"
                            } else {
                                field
                            }
                        ),
                    ));
                }
            }
        }
        if login {
            self.form(request)?;
        }
        Ok(())
    }
    fn form(&self, request: &RequestOrigins<'_>) -> BlueprintResult<()> {
        if request.cookie_present {
            return self.validate_origin(request, false);
        }
        let metadata = [request.fetch_site, request.fetch_mode, request.fetch_dest]
            .into_iter()
            .flatten()
            .any(|value| !value.trim().is_empty());
        if metadata {
            if request.fetch_site == Some("cross-site") && request.fetch_mode == Some("navigate") {
                return Err(BlueprintError::new(
                    "CROSS_SITE_NAVIGATION_LOGIN_BLOCKED",
                    "Cross-site navigation login blocked. This request appears to be a CSRF attack.",
                ));
            }
            return self.validate_origin(request, true);
        }
        if request.origin.is_some_and(|value| !value.is_empty())
            || request.referer.is_some_and(|value| !value.is_empty())
        {
            return self.validate_origin(request, true);
        }
        Ok(())
    }
    fn validate_origin(&self, request: &RequestOrigins<'_>, force: bool) -> BlueprintResult<()> {
        if !(force || request.cookie_present) {
            return Ok(());
        }
        let inferred = (request.origin == Some("null")
            && request.fetch_site == Some("same-origin"))
        .then(|| {
            Url::parse(request.request_origin)
                .ok()
                .and_then(|url| origin(&url))
        })
        .flatten();
        let candidate = inferred
            .as_deref()
            .or(request.origin.filter(|value| !value.is_empty()))
            .or(request.referer.filter(|value| !value.is_empty()))
            .unwrap_or("");
        if candidate.is_empty() || candidate == "null" {
            return Err(BlueprintError::new(
                "MISSING_OR_NULL_ORIGIN",
                "Missing or null Origin",
            ));
        }
        if !self.trusted(candidate, false) {
            return Err(BlueprintError::new("INVALID_ORIGIN", "Invalid origin"));
        }
        Ok(())
    }
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        Value::String(value) => !value.is_empty(),
        _ => true,
    }
}
fn invalid_config() -> BlueprintError {
    BlueprintError::new(
        "better_auth_invalid_config",
        "Invalid authentication base URL",
    )
}
