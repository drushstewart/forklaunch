//! Configured Better Auth rolling idle-window limits and custom counter storage.
mod storage;
use super::{AuthHttp, failure, request::client_ip};
use crate::{origins::CompiledGlob, store::AuthStore};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    middleware::Next,
    response::Response,
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
};
pub use storage::AuthRateLimitStorageDecision;

const MAX_ENTRIES: usize = 100_000;

/// A positive rolling idle-window limit. Windows are expressed in seconds.
#[derive(Clone, Copy, Debug)]
pub struct AuthRateLimitRule {
    window_seconds: u32,
    max: u32,
}
impl AuthRateLimitRule {
    /// Constructs a rolling idle-window limit.
    ///
    /// # Errors
    /// Rejects zero windows and zero request budgets at configuration time.
    pub fn new(window_seconds: u32, max: u32) -> BlueprintResult<Self> {
        if window_seconds == 0 || max == 0 {
            return Err(BlueprintError::new(
                "invalid_rate_limit",
                "Rate limits must be positive",
            ));
        }
        Ok(Self {
            window_seconds,
            max,
        })
    }
    #[must_use]
    pub fn window_seconds(self) -> u32 {
        self.window_seconds
    }
    #[must_use]
    pub fn max_requests(self) -> u32 {
        self.max
    }
    fn rule(self) -> Rule {
        Rule {
            window_ms: i64::from(self.window_seconds) * 1000,
            max: self.max,
        }
    }
}
/// Request metadata available to an asynchronous rate-limit rule.
/// The body remains untouched for downstream parsing and authentication.
#[derive(Clone, Debug)]
pub struct AuthRateLimitRequest {
    pub method: Method,
    pub uri: Uri,
    pub path: String,
    pub headers: HeaderMap,
}
/// Result of a request-specific rule, corresponding to upstream undefined,
/// false, or a replacement window/budget respectively.
#[derive(Clone, Copy, Debug)]
pub enum AuthRateLimitDecision {
    UseDefault,
    Disable,
    Limit(AuthRateLimitRule),
}
type CallbackFuture = Pin<Box<dyn Future<Output = BlueprintResult<AuthRateLimitDecision>> + Send>>;
#[derive(Clone)]
struct Callback(
    Arc<dyn Fn(AuthRateLimitRequest, AuthRateLimitRule) -> CallbackFuture + Send + Sync>,
);
impl std::fmt::Debug for Callback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RateLimitCallback")
    }
}
/// Better Auth limits with process-local or application-owned counter storage.
/// The first matching custom rule takes precedence over sensitive and default rules.
#[derive(Clone, Debug)]
pub struct AuthRateLimitOptions {
    default: AuthRateLimitRule,
    overrides: Vec<PathRule>,
    plugin_rules: Vec<PluginRule>,
    storage: Option<storage::CustomStorage>,
}
#[derive(Clone)]
struct PluginRule {
    matcher: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    rule: AuthRateLimitRule,
}
impl std::fmt::Debug for PluginRule {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PluginRateLimitRule")
            .field("rule", &self.rule)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Debug)]
struct PathRule {
    path: String,
    pattern: Option<CompiledGlob>,
    rule: Option<AuthRateLimitRule>,
    callback: Option<Callback>,
}
impl Default for AuthRateLimitOptions {
    fn default() -> Self {
        Self {
            default: AuthRateLimitRule {
                window_seconds: 10,
                max: 100,
            },
            overrides: Vec::new(),
            plugin_rules: Vec::new(),
            storage: None,
        }
    }
}
impl AuthRateLimitOptions {
    /// Installs an asynchronous counter backend. It receives the normalized
    /// `IP|auth-path` key and effective rule after plugin/custom overrides.
    /// The backend must atomically check/increment and own its expiry policy.
    /// Disabled limits skip storage; backend failures fail closed without
    /// exposing their details. There is no fallback to process-local counters.
    #[must_use]
    pub fn with_custom_storage<F, Fut>(mut self, consume: F) -> Self
    where
        F: Fn(String, AuthRateLimitRule) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<AuthRateLimitStorageDecision>> + Send + 'static,
    {
        self.storage = Some(storage::CustomStorage::new(consume));
        self
    }
    /// Adds a plugin rule in plugin/registration order. The first matching
    /// plugin rule overrides the default and sensitive-endpoint rules, before
    /// custom static or asynchronous rules are evaluated.
    #[must_use]
    pub fn with_plugin_rule(
        mut self,
        rule: AuthRateLimitRule,
        matcher: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.plugin_rules.push(PluginRule {
            matcher: Arc::new(matcher),
            rule,
        });
        self
    }
    #[must_use]
    pub fn with_default(mut self, rule: AuthRateLimitRule) -> Self {
        self.default = rule;
        self
    }
    /// Paths are relative to the auth mount. `None` disables the endpoint limit.
    ///
    /// # Errors
    /// Wildcards, queries and noncanonical paths are rejected explicitly.
    pub fn with_exact_path(
        mut self,
        path: impl Into<String>,
        rule: Option<AuthRateLimitRule>,
    ) -> BlueprintResult<Self> {
        let path = path.into();
        if !path.starts_with('/')
            || (path.len() > 1 && path.ends_with('/'))
            || path.contains(['*', '?', '#', '%'])
            || path.contains("//")
        {
            return Err(BlueprintError::new(
                "invalid_rate_limit_path",
                "Expected a canonical exact auth path",
            ));
        }
        self.insert(PathRule {
            path,
            pattern: None,
            rule,
            callback: None,
        });
        Ok(self)
    }
    /// Registers a Better Auth wildcard rule in first-match insertion order.
    /// `*` matches within a segment; a whole `**` segment spans path segments.
    /// Patterns containing `*` also interpret `?` and backslash escapes.
    /// Replacing an existing pattern retains its original position.
    ///
    /// # Errors
    /// Rejects non-auth-relative, encoded, fragment-bearing or oversized patterns.
    pub fn with_path_pattern(
        mut self,
        path: impl Into<String>,
        rule: Option<AuthRateLimitRule>,
    ) -> BlueprintResult<Self> {
        let path = path.into();
        if !path.contains('*') {
            return self.with_exact_path(path, rule);
        }
        if !path.starts_with('/') || path.contains(['%', '#']) || path.len() > 4096 {
            return Err(BlueprintError::new(
                "invalid_rate_limit_path",
                "Expected an auth-relative wildcard pattern",
            ));
        }
        let pattern = CompiledGlob::new(&path).map_err(|_| {
            BlueprintError::new("invalid_rate_limit_path", "Invalid auth wildcard pattern")
        })?;
        self.insert(PathRule {
            path,
            pattern: Some(pattern),
            rule,
            callback: None,
        });
        Ok(self)
    }
    fn insert(&mut self, rule: PathRule) {
        if let Some(existing) = self
            .overrides
            .iter_mut()
            .find(|existing| existing.path == rule.path)
        {
            *existing = rule;
        } else {
            self.overrides.push(rule);
        }
    }
    /// Registers an asynchronous exact or wildcard request rule. The callback
    /// receives the effective plugin/built-in/default rule and runs before any counter
    /// lock is acquired. Only the first matching custom rule is evaluated.
    ///
    /// # Errors
    /// Rejects unsupported paths/patterns as `with_path_pattern` does.
    pub fn with_dynamic_path<F, Fut>(
        self,
        path: impl Into<String>,
        callback: F,
    ) -> BlueprintResult<Self>
    where
        F: Fn(AuthRateLimitRequest, AuthRateLimitRule) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = BlueprintResult<AuthRateLimitDecision>> + Send + 'static,
    {
        let path = path.into();
        let mut options = self.with_path_pattern(path.clone(), None)?;
        let entry = options
            .overrides
            .iter_mut()
            .find(|entry| entry.path == path)
            .ok_or_else(internal)?;
        entry.callback = Some(Callback(Arc::new(move |request, rule| {
            Box::pin(callback(request, rule))
        })));
        Ok(options)
    }
    fn baseline(&self, path: &str) -> AuthRateLimitRule {
        if let Some(plugin) = self
            .plugin_rules
            .iter()
            .find(|plugin| (plugin.matcher)(path))
        {
            return plugin.rule;
        }
        Rule::special(path).map_or(self.default, |rule| AuthRateLimitRule {
            window_seconds: u32::try_from(rule.window_ms / 1000)
                .expect("fixed sensitive rule window"),
            max: rule.max,
        })
    }
    fn matching(&self, path: &str) -> Option<&PathRule> {
        self.overrides.iter().find(|entry| {
            entry
                .pattern
                .as_ref()
                .map_or_else(|| entry.path == path, |pattern| pattern.matches(path))
        })
    }
    async fn resolve(&self, request: AuthRateLimitRequest) -> BlueprintResult<Option<Rule>> {
        let baseline = self.baseline(&request.path);
        let Some(entry) = self.matching(&request.path) else {
            return Ok(Some(baseline.rule()));
        };
        let Some(callback) = &entry.callback else {
            return Ok(entry.rule.map(AuthRateLimitRule::rule));
        };
        Ok(match (callback.0)(request, baseline).await? {
            AuthRateLimitDecision::UseDefault => Some(baseline.rule()),
            AuthRateLimitDecision::Disable => None,
            AuthRateLimitDecision::Limit(rule) => Some(rule.rule()),
        })
    }
    fn for_path(&self, path: &str) -> Option<Rule> {
        // Upstream evaluates plugin predicates even when a static custom rule
        // ultimately overrides the result.
        let baseline = self.baseline(path).rule();
        self.matching(path).map_or_else(
            || Some(baseline),
            |entry| entry.rule.map(AuthRateLimitRule::rule),
        )
    }
}

#[derive(Clone)]
pub(super) struct RateLimiter {
    pub enabled: bool,
    pub options: AuthRateLimitOptions,
}
impl RateLimiter {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("NODE_ENV").is_ok_and(|value| value == "production"),
            options: AuthRateLimitOptions::default(),
        }
    }
    #[cfg(test)]
    fn consume(&self, ip: &str, path: &str, now: i64) -> BlueprintResult<Option<u64>> {
        if !self.enabled {
            return Ok(None);
        }
        let path = normalized_path(path);
        self.consume_rule(ip, path, self.options.for_path(path), now)
    }
    async fn consume_backend(
        &self,
        ip: &str,
        path: &str,
        rule: Option<Rule>,
        now: i64,
    ) -> BlueprintResult<Option<u64>> {
        if !self.enabled {
            return Ok(None);
        }
        let Some(rule) = rule else {
            return Ok(None);
        };
        if let Some(storage) = &self.options.storage {
            let ip = if ip.is_empty() { "no-trusted-ip" } else { ip };
            let public_rule = AuthRateLimitRule {
                window_seconds: u32::try_from(rule.window_ms / 1000).map_err(|_| internal())?,
                max: rule.max,
            };
            return storage
                .consume(format!("{ip}|{path}"), public_rule)
                .await
                .map_err(|_| internal());
        }
        self.consume_rule(ip, path, Some(rule), now)
    }
    fn consume_rule(
        &self,
        ip: &str,
        path: &str,
        rule: Option<Rule>,
        now: i64,
    ) -> BlueprintResult<Option<u64>> {
        static MEMORY: OnceLock<Mutex<Memory>> = OnceLock::new();
        if !self.enabled {
            return Ok(None);
        }
        let Some(rule) = rule else {
            return Ok(None);
        };
        // Better Auth shares IP|normalized-path counters across auth instances.
        // User callbacks run before this lock; check and increment remain atomic.
        let mut state = MEMORY
            .get_or_init(|| Mutex::new(Memory::default()))
            .lock()
            .map_err(|_| internal())?;
        let ip = if ip.is_empty() { "no-trusted-ip" } else { ip };
        state.consume(Arc::from(format!("{ip}|{path}")), rule, now, MAX_ENTRIES)
    }
}

fn normalized_path(path: &str) -> &str {
    let path = path.trim_end_matches('/');
    if path.is_empty() { "/" } else { path }
}

pub(super) async fn guard<S: AuthStore + 'static>(
    State(app): State<Arc<AuthHttp<S>>>,
    request: Request,
    next: Next,
) -> Response {
    if !app.limiter.enabled {
        return next.run(request).await;
    }
    let ip = client_ip(request.headers());
    let path = normalized_path(request.uri().path()).to_owned();
    let rule = if app
        .limiter
        .options
        .matching(&path)
        .is_some_and(|entry| entry.callback.is_some())
    {
        let metadata = AuthRateLimitRequest {
            method: request.method().clone(),
            uri: request
                .extensions()
                .get::<axum::extract::OriginalUri>()
                .map_or_else(|| request.uri().clone(), |original| original.0.clone()),
            path: path.clone(),
            headers: request.headers().clone(),
        };
        match app.limiter.options.resolve(metadata).await {
            Ok(rule) => rule,
            Err(_) => return failure(&internal()),
        }
    } else {
        app.limiter.options.for_path(&path)
    };
    match app
        .limiter
        .consume_backend(&ip, &path, rule, (app.clock)().timestamp_millis())
        .await
    {
        Ok(None) => next.run(request).await,
        Ok(Some(retry)) => {
            let mut response = Response::new(Body::from(
                r#"{"message":"Too many requests. Please try again later."}"#,
            ));
            *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain;charset=UTF-8"),
            );
            match HeaderValue::from_str(&retry.to_string()) {
                Ok(value) => {
                    response.headers_mut().insert("x-retry-after", value);
                    response
                }
                Err(_) => failure(&internal()),
            }
        }
        Err(error) => failure(&error),
    }
}

#[derive(Clone, Copy)]
struct Rule {
    window_ms: i64,
    max: u32,
}
impl Rule {
    fn special(path: &str) -> Option<Self> {
        if ["/sign-in", "/sign-up", "/change-password", "/change-email"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            Some(Self {
                window_ms: 10_000,
                max: 3,
            })
        } else if [
            "/request-password-reset",
            "/send-verification-email",
            "/email-otp/send-verification-otp",
            "/email-otp/request-password-reset",
        ]
        .contains(&path)
            || path.starts_with("/forget-password")
        {
            Some(Self {
                window_ms: 60_000,
                max: 3,
            })
        } else {
            None
        }
    }
}
#[derive(Clone, Copy)]
struct Entry {
    count: u32,
    last: i64,
    expires: i64,
    ordinal: u64,
}
/// Expiry and insertion indexes stay bounded alongside the primary map. Unlike
/// scanning every bucket on each request, pruning costs O(expired * log(n)).
#[derive(Default)]
struct Memory {
    entries: HashMap<Arc<str>, Entry>,
    expiry: BTreeMap<(i64, u64), Arc<str>>,
    order: BTreeMap<u64, Arc<str>>,
    next_ordinal: u64,
}
impl Memory {
    fn consume(
        &mut self,
        key: Arc<str>,
        rule: Rule,
        now: i64,
        capacity: usize,
    ) -> BlueprintResult<Option<u64>> {
        self.prune(now, capacity);
        let previous = self.entries.get(&key).copied();
        let within = previous.is_some_and(|entry| now.saturating_sub(entry.last) < rule.window_ms);
        if let Some(previous) = previous
            && within
            && previous.count >= rule.max
        {
            let remaining = previous
                .last
                .checked_add(rule.window_ms)
                .and_then(|end| end.checked_sub(now))
                .ok_or_else(internal)?;
            return Ok(Some(
                u64::try_from(remaining)
                    .map_err(|_| internal())?
                    .div_ceil(1000),
            ));
        }
        let ordinal = if let Some(previous) = previous {
            previous.ordinal
        } else {
            let ordinal = self.next_ordinal;
            self.next_ordinal = ordinal.checked_add(1).ok_or_else(internal)?;
            ordinal
        };
        let expires = now.checked_add(rule.window_ms).ok_or_else(internal)?;
        let count = if within {
            previous.ok_or_else(internal)?.count + 1
        } else {
            1
        };
        if let Some(previous) = previous {
            self.expiry.remove(&(previous.expires, previous.ordinal));
        }
        self.expiry.insert((expires, ordinal), key.clone());
        self.order.insert(ordinal, key.clone());
        self.entries.insert(
            key,
            Entry {
                count,
                last: now,
                expires,
                ordinal,
            },
        );
        Ok(None)
    }
    fn prune(&mut self, now: i64, capacity: usize) {
        while self
            .expiry
            .first_key_value()
            .is_some_and(|((expires, _), _)| *expires <= now)
        {
            if let Some(((_, ordinal), key)) = self.expiry.pop_first() {
                self.entries.remove(&key);
                self.order.remove(&ordinal);
            }
        }
        // The actual implementation prunes before consuming, so a new bucket
        // may exceed the limit by one until the following request.
        while self.entries.len() > capacity {
            if let Some((ordinal, key)) = self.order.pop_first()
                && let Some(entry) = self.entries.remove(&key)
            {
                self.expiry.remove(&(entry.expires, ordinal));
            }
        }
    }
}
fn internal() -> BlueprintError {
    BlueprintError::new(
        "better_auth_rate_limit_error",
        "Authentication rate limit operation failed",
    )
}

pub(super) const fn two_factor_rule() -> AuthRateLimitRule {
    AuthRateLimitRule {
        window_seconds: 10,
        max: 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_rules_preserve_special_limits_and_allow_exact_overrides() {
        assert!(AuthRateLimitRule::new(0, 1).is_err());
        assert!(AuthRateLimitRule::new(1, 0).is_err());
        for path in ["ok", "/sign-in/*", "/ok/", "/ok?x=1", "/%6fk", "//ok"] {
            assert!(
                AuthRateLimitOptions::default()
                    .with_exact_path(path, None)
                    .is_err()
            );
        }
        let options = AuthRateLimitOptions::default()
            .with_default(AuthRateLimitRule::new(20, 2).unwrap())
            .with_exact_path(
                "/sign-in/email",
                Some(AuthRateLimitRule::new(30, 5).unwrap()),
            )
            .unwrap()
            .with_exact_path("/sign-up/email", None)
            .unwrap();
        assert_eq!(options.for_path("/ok").unwrap().window_ms, 20_000);
        assert_eq!(options.for_path("/ok").unwrap().max, 2);
        assert_eq!(options.for_path("/sign-in/email").unwrap().max, 5);
        assert_eq!(options.for_path("/sign-in/social").unwrap().max, 3);
        assert_eq!(
            options
                .for_path("/request-password-reset")
                .unwrap()
                .window_ms,
            60_000
        );
        assert!(options.for_path("/sign-up/email").is_none());
    }
    #[test]
    fn wildcard_first_match_replacement_and_segment_boundaries() {
        let rule = AuthRateLimitRule::new(8, 2).unwrap();
        let options = AuthRateLimitOptions::default()
            .with_path_pattern("/sign-in/*", Some(rule))
            .unwrap()
            .with_exact_path("/sign-in/email", None)
            .unwrap();
        assert_eq!(options.for_path("/sign-in/email").unwrap().max, 2);
        assert_eq!(options.for_path("/sign-in/deep/missing").unwrap().max, 3);
        let options = options
            .with_path_pattern("/sign-in/*", None)
            .unwrap()
            .with_path_pattern("/sign-in/**", Some(rule))
            .unwrap();
        assert!(options.for_path("/sign-in/email").is_none());
        assert_eq!(options.for_path("/sign-in/deep/missing").unwrap().max, 2);
        let options = AuthRateLimitOptions::default()
            .with_exact_path("/sign-in/email", None)
            .unwrap()
            .with_path_pattern("/**", Some(rule))
            .unwrap();
        assert!(options.for_path("/sign-in/email").is_none());
        assert_eq!(options.for_path("/ok").unwrap().max, 2);
        for path in ["sign-in/*", "/%73ign-in/*", "/sign-in/*#x"] {
            assert!(
                AuthRateLimitOptions::default()
                    .with_path_pattern(path, None)
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn custom_storage_is_async_atomic_and_skipped_for_disabled_limits() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let options = AuthRateLimitOptions::default().with_custom_storage(move |key, rule| {
            let observed = observed.clone();
            async move {
                tokio::task::yield_now().await;
                assert_eq!(key, "no-trusted-ip|/sign-in/email");
                assert_eq!(rule.window_seconds(), 10);
                let count = observed.fetch_add(1, Ordering::SeqCst);
                Ok(if count < rule.max_requests() as usize {
                    AuthRateLimitStorageDecision::Allow
                } else {
                    AuthRateLimitStorageDecision::Deny { retry_after: None }
                })
            }
        });
        let limiter = Arc::new(RateLimiter {
            enabled: true,
            options: options.clone(),
        });
        let rule = Rule::special("/sign-in/email");
        let mut workers = Vec::new();
        for _ in 0..16 {
            let limiter = limiter.clone();
            workers.push(tokio::spawn(async move {
                limiter
                    .consume_backend("", "/sign-in/email", rule, 0)
                    .await
                    .unwrap()
            }));
        }
        let mut admitted = 0;
        for worker in workers {
            match worker.await.unwrap() {
                None => admitted += 1,
                Some(retry) => assert_eq!(retry, 10),
            }
        }
        assert_eq!(admitted, 3);
        assert_eq!(
            limiter
                .consume_backend("", "/sign-in/email", None, 0)
                .await
                .unwrap(),
            None
        );
        let disabled = RateLimiter {
            enabled: false,
            options,
        };
        assert_eq!(
            disabled
                .consume_backend("", "/sign-in/email", rule, 0)
                .await
                .unwrap(),
            None
        );
        assert_eq!(calls.load(Ordering::SeqCst), 16);
    }
    #[tokio::test]
    async fn custom_storage_failures_are_private_and_never_fall_back() {
        use crate::store::memory::MemoryAuthStore;
        use tower::ServiceExt;
        let options = AuthRateLimitOptions::default().with_custom_storage(|_, _| async {
            Err(BlueprintError::new(
                "private-code",
                "private-storage-secret",
            ))
        });
        let router = AuthHttp::new(
            Arc::new(MemoryAuthStore::default()),
            "http://localhost:3000".into(),
            vec![42; 32],
            vec![],
        )
        .unwrap()
        .with_rate_limiting(true)
        .with_rate_limit_options(options)
        .router();
        let response = router
            .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("private-"));
    }
    #[tokio::test]
    async fn plugin_order_precedes_custom_rules_and_supplies_callback_defaults() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let options = AuthRateLimitOptions::default()
            .with_plugin_rule(AuthRateLimitRule::new(5, 1).unwrap(), move |path| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                path.starts_with("/sign-in")
            })
            .with_plugin_rule(AuthRateLimitRule::new(6, 2).unwrap(), |_| true)
            .with_exact_path("/sign-up/email", None)
            .unwrap()
            .with_dynamic_path("/sign-in/email", |_, baseline| async move {
                assert_eq!(baseline.window_seconds(), 5);
                assert_eq!(baseline.max_requests(), 1);
                Ok(AuthRateLimitDecision::UseDefault)
            })
            .unwrap();
        assert_eq!(options.for_path("/ok").unwrap().max, 2);
        assert_eq!(options.for_path("/sign-in/social").unwrap().max, 1);
        assert!(options.for_path("/sign-up/email").is_none());
        let rule = options
            .resolve(AuthRateLimitRequest {
                method: Method::POST,
                uri: "/sign-in/email".parse().unwrap(),
                path: "/sign-in/email".into(),
                headers: HeaderMap::new(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rule.window_ms, 5000);
        assert_eq!(rule.max, 1);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
    }
    #[tokio::test]
    async fn async_rules_preserve_atomicity_and_run_without_counter_locks() {
        let path = format!("/sign-in/{}", uuid::Uuid::new_v4());
        let barrier = Arc::new(tokio::sync::Barrier::new(16));
        let options = AuthRateLimitOptions::default()
            .with_dynamic_path(&path, move |request, baseline| {
                let barrier = barrier.clone();
                async move {
                    assert_eq!(request.method, Method::POST);
                    assert_eq!(request.uri.query(), Some("client=fixture"));
                    assert_eq!(baseline.max_requests(), 3);
                    barrier.wait().await;
                    Ok(AuthRateLimitDecision::UseDefault)
                }
            })
            .unwrap();
        let limiter = Arc::new(RateLimiter {
            enabled: true,
            options,
        });
        let mut workers = Vec::new();
        for _ in 0..16 {
            let limiter = limiter.clone();
            let path = path.clone();
            workers.push(tokio::spawn(async move {
                let request = AuthRateLimitRequest {
                    method: Method::POST,
                    uri: "/api/auth/sign-in/test?client=fixture".parse().unwrap(),
                    path: path.clone(),
                    headers: HeaderMap::new(),
                };
                let rule = limiter.options.resolve(request).await.unwrap();
                limiter
                    .consume_rule("192.0.2.240", &path, rule, 1_800_000_000_000)
                    .unwrap()
                    .is_none()
            }));
        }
        let mut admitted = 0;
        for worker in workers {
            admitted += usize::from(worker.await.unwrap());
        }
        assert_eq!(admitted, 3);
    }
    #[tokio::test]
    async fn callback_errors_do_not_fall_through_to_another_rule() {
        let options = AuthRateLimitOptions::default()
            .with_dynamic_path("/sign-in/*", |_, _| async {
                Err(BlueprintError::new("fixture_failure", "private failure"))
            })
            .unwrap()
            .with_path_pattern("/**", None)
            .unwrap();
        let result = options
            .resolve(AuthRateLimitRequest {
                method: Method::POST,
                uri: "/sign-in/email".parse().unwrap(),
                path: "/sign-in/email".into(),
                headers: HeaderMap::new(),
            })
            .await;
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn callback_failure_is_private_and_disabled_limiter_skips_callbacks() {
        use crate::store::memory::MemoryAuthStore;
        use tower::ServiceExt;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let options = AuthRateLimitOptions::default()
            .with_dynamic_path("/**", move |_, _| {
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async { Err(BlueprintError::new("private-code", "private-secret")) }
            })
            .unwrap();
        for enabled in [true, false] {
            let router = AuthHttp::new(
                Arc::new(MemoryAuthStore::default()),
                "http://localhost:3000".into(),
                vec![42; 32],
                vec![],
            )
            .unwrap()
            .with_rate_limiting(enabled)
            .with_rate_limit_options(options.clone())
            .router();
            let router = axum::Router::new().nest("/api/auth", router);
            let response = router
                .oneshot(
                    Request::builder()
                        .uri("/api/auth/ok")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if enabled {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::OK
                }
            );
            let bytes = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("private-"));
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn rolling_expiry_denials_and_insertion_eviction_keep_indexes_bounded() {
        let mut memory = Memory::default();
        let rule = Rule {
            window_ms: 10_000,
            max: 3,
        };
        for now in [0, 5000, 9000] {
            assert_eq!(memory.consume(Arc::from("a"), rule, now, 2).unwrap(), None);
        }
        assert_eq!(
            memory.consume(Arc::from("a"), rule, 10_000, 2).unwrap(),
            Some(9)
        );
        assert_eq!(
            memory.consume(Arc::from("a"), rule, 18_999, 2).unwrap(),
            Some(1)
        );
        assert_eq!(
            memory.consume(Arc::from("a"), rule, 19_000, 2).unwrap(),
            None
        );
        assert_eq!(memory.entries["a"].count, 1);
        memory.consume(Arc::from("b"), rule, 19_000, 2).unwrap();
        memory.consume(Arc::from("a"), rule, 20_000, 2).unwrap();
        memory.consume(Arc::from("c"), rule, 20_000, 2).unwrap();
        assert_eq!(memory.entries.len(), 3);
        memory.consume(Arc::from("c"), rule, 20_001, 2).unwrap();
        assert!(
            !memory.entries.contains_key("a"),
            "updates retain insertion order"
        );
        assert_eq!(memory.entries.len(), 2);
        assert_eq!(memory.expiry.len(), 2);
        assert_eq!(memory.order.len(), 2);
        memory.prune(30_001, 2);
        assert!(memory.entries.is_empty());
        assert!(memory.expiry.is_empty());
        assert!(memory.order.is_empty());
    }
    #[test]
    fn concurrent_default_special_rule_allows_exactly_three_requests() {
        let path = format!("/sign-in/{}", uuid::Uuid::new_v4());
        let workers: Vec<_> = (0..32)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    RateLimiter {
                        enabled: true,
                        options: AuthRateLimitOptions::default(),
                    }
                    .consume("192.0.2.1", &path, 1_800_000_000_000)
                    .unwrap()
                    .is_none()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| usize::from(worker.join().unwrap()))
                .sum::<usize>(),
            3
        );
    }
}
