//! Optional TOTP and delivered OTP profiles. All mutation routes share the existing origin guard.
use super::{
    Arc, AuthHttp, AuthStore, BlueprintError, BlueprintResult, DateTime, HeaderMap, Router, Utc,
    Value, dispatch, internal, json, post, request, set_cookie, value,
};
use crate::{
    cookies::{cookie_value, sign_cookie_value, verify_cookie_value},
    two_factor::{BackupVerificationOptions, FactorResponse, TwoFactorService},
};
const PATHS: [&str; 8] = [
    "/two-factor/send-otp",
    "/two-factor/verify-otp",
    "/two-factor/enable",
    "/two-factor/get-totp-uri",
    "/two-factor/verify-totp",
    "/two-factor/disable",
    "/two-factor/generate-backup-codes",
    "/two-factor/verify-backup-code",
];
impl<S: AuthStore + 'static> AuthHttp<S> {
    /// Enables fixed-default delivered OTP with a trusted server-owned delivery hook.
    /// This does not configure a network channel or accept delivery targets from HTTP.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted auth configuration.
    pub fn with_two_factor_otp(
        self,
        delivery: Arc<dyn crate::two_factor::OtpDelivery>,
    ) -> BlueprintResult<Self> {
        self.with_two_factor_otp_options(delivery, crate::two_factor::OtpOptions::default())
    }
    /// Enables delivered OTP with validated trusted policy. Existing code expiry
    /// is persisted independently; verification uses this current attempt policy.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted auth configuration.
    pub fn with_two_factor_otp_options(
        mut self,
        delivery: Arc<dyn crate::two_factor::OtpDelivery>,
        options: crate::two_factor::OtpOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_otp_delivery(delivery)
                .with_otp_options(options),
        ));
        Ok(self)
    }

    /// Selects trusted OTP field encoding without changing delivery or row encryption.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted auth configuration.
    pub fn with_two_factor_otp_storage(
        mut self,
        storage: crate::two_factor::OtpStorage,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_otp_storage(storage),
        ));
        Ok(self)
    }

    /// Enables encrypted OTP field storage with independently configured keys.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_otp_encryption_keys(
        mut self,
        keys: Arc<crate::two_factor::OtpEncryptionKeys>,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_otp_encryption_keys(keys),
        ));
        Ok(self)
    }

    /// Installs a server-owned asynchronous OTP hash codec. No request selects this policy.
    /// # Errors
    /// Rejects invalid existing authentication configuration or namespace conflicts.
    pub fn with_two_factor_otp_hash_codec(
        mut self,
        codec: Arc<dyn crate::two_factor::OtpHashCodec>,
        options: crate::two_factor::OtpCodecOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_otp_hash_codec(codec, options),
        ));
        Ok(self)
    }
    /// Installs a server-owned asynchronous OTP encryption codec. No request selects this policy.
    /// # Errors
    /// Rejects invalid existing authentication configuration or namespace conflicts.
    pub fn with_two_factor_otp_encryption_codec(
        mut self,
        codec: Arc<dyn crate::two_factor::OtpEncryptionCodec>,
        options: crate::two_factor::OtpCodecOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_otp_encryption_codec(codec, options),
        ));
        Ok(self)
    }
    /// Enables TOTP using validated trusted parameters, preserving any configured
    /// delivered-OTP hook and policy. This does not enable passwordless enrollment.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted auth configuration.
    pub fn with_two_factor_totp_options(
        mut self,
        options: crate::two_factor::TotpOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_totp_options(options),
        ));
        Ok(self)
    }

    /// Configures newly generated backup codes; existing codes remain valid.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_backup_code_options(
        mut self,
        options: crate::two_factor::BackupCodeOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_backup_code_options(options),
        ));
        Ok(self)
    }
    /// Installs a trusted synchronous generator overriding numeric generation options.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_backup_code_generator(
        mut self,
        generator: Arc<dyn crate::two_factor::BackupCodeGenerator>,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_backup_code_generator(generator),
        ));
        Ok(self)
    }

    /// Configures the pending-sign-in account budget independently of per-proof limits.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_account_lockout_options(
        mut self,
        options: crate::two_factor::AccountLockoutOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_account_lockout_options(options),
        ));
        Ok(self)
    }

    /// Configures challenge/trusted-device expiry without changing session or OTP TTLs.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_lifetime_options(
        mut self,
        options: crate::two_factor::TwoFactorLifetimeOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_lifetime_options(options),
        ));
        Ok(self)
    }

    /// Enables trusted lifecycle policy while preserving OTP/TOTP configuration.
    /// Passwordless never skips an existing credential password, and policy is not
    /// accepted from HTTP. Skip-verification deliberately enables protection immediately.
    /// # Errors
    /// Rejects conflicting JWKS namespace or invalid trusted authentication configuration.
    pub fn with_two_factor_lifecycle_options(
        mut self,
        options: crate::two_factor::TwoFactorLifecycleOptions,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_lifecycle_options(options),
        ));
        Ok(self)
    }

    pub(super) fn configured_schema(&self) -> Value {
        let mut schema = super::public::openapi_schema(&self.auth_base_url(), self.jwks_path());
        if self.two_factor.is_some() {
            augment_schema(&mut schema);
            if let Some(service) = &self.two_factor {
                for path in PATHS {
                    if service.lifecycle_options().password_optional(path) {
                        schema["paths"][path]["post"]["requestBody"]["content"]["application/json"]
                            ["schema"]["required"] = json!([]);
                    }
                }
            }
            if self
                .two_factor
                .as_ref()
                .is_some_and(|service| service.has_otp_delivery())
            {
                schema["paths"]["/two-factor/enable"]["post"]["requestBody"]["content"]["application/json"]
                    ["schema"]["properties"]["method"]["enum"] = json!(["totp", "otp"]);
                let previous = schema["paths"]["/two-factor/enable"]["post"]["responses"]["200"]["content"]["application/json"]["schema"].take();
                schema["paths"]["/two-factor/enable"]["post"]["responses"]["200"]["content"]["application/json"]
                    ["schema"] = json!({"oneOf":[previous,{"type":"object","required":["method"],"properties":{"method":{"type":"string","enum":["otp"]}}}]});
            }
        }
        schema
    }
    /// Selects trusted backup field storage. The first MFA opt-in otherwise uses
    /// encrypted JSON; later builders preserve this explicit selection.
    /// # Errors
    /// Rejects invalid MFA configuration or conflicting route namespaces.
    pub fn with_two_factor_backup_code_storage(
        mut self,
        storage: crate::two_factor::BackupCodeStorage,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_backup_code_storage(storage),
        ));
        Ok(self)
    }
    /// Selects encrypted backup fields with independent trusted keys.
    /// # Errors
    /// Rejects invalid MFA configuration or conflicting route namespaces.
    pub fn with_two_factor_backup_code_encryption_keys(
        mut self,
        keys: Arc<crate::two_factor::BackupCodeEncryptionKeys>,
    ) -> BlueprintResult<Self> {
        self = self.with_two_factor()?;
        self.two_factor = Some(Arc::new(
            self.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .copy_configuration()
                .with_backup_code_encryption_keys(keys),
        ));
        Ok(self)
    }
    /// Enables the fixed default TOTP/backup/trusted-device profile for password
    /// sign-in. Social callbacks remain independently governed, as upstream.
    /// # Errors
    /// Rejects a conflicting JWKS namespace or invalid trusted signing configuration.
    pub fn with_two_factor(mut self) -> BlueprintResult<Self> {
        if self.jwks_path.starts_with("/two-factor/") || self.jwks_path == "/two-factor" {
            return Err(BlueprintError::new(
                "better_auth_invalid_config",
                "JWKS path conflicts with two-factor routes",
            ));
        }
        if self.two_factor.is_none() {
            self.two_factor = Some(Arc::new(
                TwoFactorService::new(
                    self.store.clone(),
                    self.secret.to_vec(),
                    "Better Auth".into(),
                )?
                .with_backup_code_storage(crate::two_factor::BackupCodeStorage::Encrypted),
            ));
        }
        Ok(self)
    }
}
pub(super) fn routes<S: AuthStore + 'static>(enabled: bool) -> Router<Arc<AuthHttp<S>>> {
    let mut router = Router::new();
    if enabled {
        for path in PATHS {
            router = router.route(path, post(dispatch::<S>));
        }
    }
    router
}
pub(super) fn validate(
    path: &str,
    body: Option<&Value>,
    password_optional: bool,
) -> BlueprintResult<()> {
    if path == "/two-factor/send-otp" {
        if body.is_none() {
            return Ok(());
        }
        let body = body.filter(|value| value.is_object()).ok_or_else(|| {
            BlueprintError::new("VALIDATION_ERROR", "[body] Invalid input: expected object")
        })?;
        if body
            .get("trustDevice")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err(BlueprintError::new(
                "VALIDATION_ERROR",
                "[body.trustDevice] Invalid input: expected boolean",
            ));
        }
        return Ok(());
    }
    let body = body.filter(|value| value.is_object()).ok_or_else(|| {
        BlueprintError::new("VALIDATION_ERROR", "[body] Invalid input: expected object")
    })?;
    let field = if matches!(
        path,
        "/two-factor/verify-totp" | "/two-factor/verify-backup-code" | "/two-factor/verify-otp"
    ) {
        "code"
    } else {
        "password"
    };
    if !(body[field].is_string()
        || field == "password" && password_optional && body.get(field).is_none())
    {
        return Err(BlueprintError::new(
            "VALIDATION_ERROR",
            format!("[body.{field}] Invalid input: expected string"),
        ));
    }
    let boolean_fields: &[&str] = match path {
        "/two-factor/verify-totp" | "/two-factor/verify-otp" => &["trustDevice"],
        "/two-factor/verify-backup-code" => &["trustDevice", "disableSession"],
        _ => &[],
    };
    for &field in boolean_fields {
        if body.get(field).is_some_and(|value| !value.is_boolean()) {
            return Err(BlueprintError::new(
                "VALIDATION_ERROR",
                format!("[body.{field}] Invalid input: expected boolean"),
            ));
        }
    }
    if path == "/two-factor/enable" && body.get("issuer").is_some_and(|value| !value.is_string()) {
        return Err(BlueprintError::new(
            "VALIDATION_ERROR",
            "[body.issuer] Invalid input: expected string",
        ));
    }
    if path == "/two-factor/enable"
        && let Some(method) = body.get("method")
        && method != "totp"
        && method != "otp"
    {
        return Err(BlueprintError::new(
            "VALIDATION_ERROR",
            "Invalid two-factor method",
        ));
    }
    Ok(())
}
pub(super) fn signed<S: AuthStore>(
    app: &AuthHttp<S>,
    headers: &HeaderMap,
    name: &str,
) -> Option<String> {
    let name = format!(
        "{}better-auth.{name}",
        if app.cookie.secure { "__Secure-" } else { "" }
    );
    cookie_value(value(headers, "cookie").unwrap_or(""), &name)
        .and_then(|value| verify_cookie_value(&value, &app.secret).map(str::to_owned))
}
pub(super) fn cookie<S: AuthStore>(
    app: &AuthHttp<S>,
    pending: &mut HeaderMap,
    name: &str,
    value: Option<&str>,
    max_age: u64,
) -> BlueprintResult<()> {
    let encoded = value
        .map(|value| sign_cookie_value(value, &app.secret))
        .unwrap_or_default();
    set_cookie(
        pending,
        &format!(
            "{}better-auth.{name}={encoded}; Max-Age={}; Path=/; HttpOnly{}; SameSite=Lax",
            if app.cookie.secure { "__Secure-" } else { "" },
            if value.is_some() { max_age } else { 0 },
            if app.cookie.secure { "; Secure" } else { "" }
        ),
    )
}
// Keep route dispatch together so built-in and async-codec clock authority stays explicit.
#[allow(clippy::too_many_lines)]
pub(super) async fn handle<S: AuthStore + 'static>(
    app: &AuthHttp<S>,
    path: &str,
    body: &Value,
    headers: &HeaderMap,
    pending: &mut HeaderMap,
    now: DateTime<Utc>,
) -> BlueprintResult<Value> {
    let service = app
        .two_factor
        .as_ref()
        .ok_or_else(|| BlueprintError::new("HTTP_NOT_FOUND", "Not Found"))?;
    let (token, transient) = request::session_credentials(app, headers);
    let trust = signed(app, headers, "trust_device");
    let password = || body["password"].as_str().unwrap_or("").to_owned();
    let result = match path {
        "/two-factor/send-otp" if service.has_custom_otp_codec() => {
            service
                .send_otp_with_clock(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    app.clock.as_ref(),
                )
                .await
        }
        "/two-factor/verify-otp" if service.has_custom_otp_codec() => {
            service
                .verify_otp_with_clock(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    body["code"].as_str().unwrap_or(""),
                    body["trustDevice"] == true,
                    app.clock.as_ref(),
                )
                .await
        }
        "/two-factor/send-otp" => {
            service
                .send_otp(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    now,
                )
                .await
        }
        "/two-factor/verify-otp" => {
            service
                .verify_otp(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    body["code"].as_str().unwrap_or(""),
                    body["trustDevice"] == true,
                    now,
                )
                .await
        }
        "/two-factor/enable" if body["method"] == "otp" => {
            service.enable_otp(&token, password(), now).await
        }
        "/two-factor/enable" => {
            service
                .enable(&token, password(), body["issuer"].as_str(), now)
                .await
        }
        "/two-factor/get-totp-uri" => service.get_uri(&token, password(), now).await,
        "/two-factor/generate-backup-codes" => {
            service
                .regenerate_backup_codes(&token, password(), now)
                .await
        }
        "/two-factor/disable" => {
            service
                .disable(&token, password(), trust.as_deref(), now)
                .await
        }
        "/two-factor/verify-backup-code" => {
            service
                .verify_backup_with_options(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    body["code"].as_str().unwrap_or(""),
                    BackupVerificationOptions {
                        disable_session: body["disableSession"] == true,
                        trust_device: body["trustDevice"] == true,
                    },
                    now,
                )
                .await
        }
        "/two-factor/verify-totp" => {
            service
                .verify(
                    &token,
                    &signed(app, headers, "two_factor").unwrap_or_default(),
                    body["code"].as_str().unwrap_or(""),
                    path.ends_with("verify-backup-code"),
                    body["trustDevice"] == true,
                    now,
                )
                .await
        }
        _ => Err(BlueprintError::new("HTTP_NOT_FOUND", "Not Found")),
    };
    match result {
        Ok(response) => apply(app, pending, response, transient),
        Err(error) => {
            if error.code == "TOO_MANY_ATTEMPTS_REQUEST_NEW_CODE" {
                cookie(app, pending, "two_factor", None, 0)?;
            }
            Err(error)
        }
    }
}
fn apply<S: AuthStore>(
    app: &AuthHttp<S>,
    pending: &mut HeaderMap,
    response: FactorResponse,
    transient: bool,
) -> BlueprintResult<Value> {
    let persistent = response.persistent && !transient;
    let replaced_session = response.session.is_some();
    if let Some(session) = response.session {
        set_cookie(
            pending,
            &app.cookie.serialize(
                session["token"].as_str().ok_or_else(internal)?,
                &app.secret,
                persistent,
            ),
        )?;
    }
    if replaced_session && !persistent && response.trust.is_none() {
        set_cookie(pending, &app.cookie.transient_flag(&app.secret))?;
    }
    if response.clear_challenge {
        cookie(app, pending, "two_factor", None, 0)?;
    }
    if response.clear_trust {
        cookie(app, pending, "trust_device", None, 0)?;
    }
    if let Some(trust) = response.trust {
        cookie(
            app,
            pending,
            "trust_device",
            Some(&trust),
            app.two_factor
                .as_ref()
                .ok_or_else(internal)?
                .lifetime_options()
                .trusted_device()
                .as_secs(),
        )?;
        set_cookie(
            pending,
            &format!(
                "{}=; Max-Age=0; Path=/; HttpOnly{}; SameSite=Lax",
                app.cookie.dont_remember_name(),
                if app.cookie.secure { "; Secure" } else { "" }
            ),
        )?;
    }
    Ok(response.body)
}
pub(super) fn user_default(body: &mut Value) {
    if let Some(user) = body.get_mut("user").and_then(Value::as_object_mut) {
        user.entry("twoFactorEnabled").or_insert(json!(false));
    }
}

pub(super) fn augment_schema(schema: &mut Value) {
    for path in PATHS {
        let field = if path.ends_with("verify-totp")
            || path.ends_with("verify-backup-code")
            || path.ends_with("verify-otp")
        {
            "code"
        } else {
            "password"
        };
        let operation = match path {
            "/two-factor/send-otp" => "sendTwoFactorOTP",
            "/two-factor/verify-otp" => "verifyTwoFactorOTP",
            "/two-factor/enable" => "enableTwoFactor",
            "/two-factor/get-totp-uri" => "getTOTPURI",
            "/two-factor/verify-totp" => "verifyTOTP",
            "/two-factor/disable" => "disableTwoFactor",
            "/two-factor/generate-backup-codes" => "generateBackupCodes",
            _ => "verifyBackupCode",
        };
        let mut properties = serde_json::Map::new();
        properties.insert(field.into(), json!({"type":"string"}));
        if field == "code" {
            properties.insert("trustDevice".into(), json!({"type":"boolean"}));
        }
        if path == "/two-factor/enable" {
            properties.insert("issuer".into(), json!({"type":"string","maxLength":256}));
            properties.insert("method".into(), json!({"type":"string","enum":["totp"]}));
        }
        let response = match path {
            "/two-factor/enable" => {
                json!({"type":"object","required":["method","totpURI","backupCodes"],"properties":{"method":{"type":"string","enum":["totp"]},"totpURI":{"type":"string"},"backupCodes":{"type":"array","items":{"type":"string"}}}})
            }
            "/two-factor/get-totp-uri" => {
                json!({"type":"object","required":["totpURI"],"properties":{"totpURI":{"type":"string"}}})
            }
            "/two-factor/generate-backup-codes" => {
                json!({"type":"object","required":["status","backupCodes"],"properties":{"status":{"type":"boolean"},"backupCodes":{"type":"array","items":{"type":"string"}}}})
            }
            "/two-factor/disable" | "/two-factor/send-otp" => {
                json!({"type":"object","required":["status"],"properties":{"status":{"type":"boolean"}}})
            }
            _ => {
                json!({"type":"object","required":["token","user"],"properties":{"token":{"type":"string"},"user":{"type":"object"}}})
            }
        };
        schema["paths"][path] = json!({"post":{"operationId":operation,"tags":["Two Factor"],"requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","required":[field],"properties":properties}}}},"responses":{"200":{"description":"Successful response","content":{"application/json":{"schema":response}}}}}});
        if path == "/two-factor/send-otp" {
            schema["paths"][path]["post"]["requestBody"] = json!({"required":false,"content":{"application/json":{"schema":{"type":"object","properties":{"trustDevice":{"type":"boolean"}}}}}});
        }
    }
    if let Some(user) = schema
        .pointer_mut("/components/schemas/User/properties")
        .and_then(Value::as_object_mut)
    {
        user.insert("twoFactorEnabled".into(), json!({"type":"boolean"}));
    }
}
