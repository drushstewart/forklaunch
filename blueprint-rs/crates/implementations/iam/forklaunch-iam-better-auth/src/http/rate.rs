//! Configured Better Auth process-local rolling idle-window rate limits.
use super::{AuthHttp, failure, request::client_ip};
use crate::store::AuthStore;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, OnceLock},
};

const MAX_ENTRIES: usize = 100_000;

#[derive(Clone, Copy)]
pub(super) struct RateLimiter {
    pub enabled: bool,
}
impl RateLimiter {
    pub fn new() -> Self {
        Self {
            enabled: std::env::var("NODE_ENV").is_ok_and(|value| value == "production"),
        }
    }
    fn consume(self, ip: &str, path: &str, now: i64) -> BlueprintResult<Option<u64>> {
        static MEMORY: OnceLock<Mutex<Memory>> = OnceLock::new();
        if !self.enabled {
            return Ok(None);
        }
        // Better Auth's default memory storage is shared across auth instances
        // in a process. Keep its exact IP|normalized-path key and insertion order.
        let mut state = MEMORY
            .get_or_init(|| Mutex::new(Memory::default()))
            .lock()
            .map_err(|_| internal())?;
        let path = path.trim_end_matches('/');
        let path = if path.is_empty() { "/" } else { path };
        let ip = if ip.is_empty() { "no-trusted-ip" } else { ip };
        state.consume(
            Arc::from(format!("{ip}|{path}")),
            Rule::for_path(path),
            now,
            MAX_ENTRIES,
        )
    }
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
    match app
        .limiter
        .consume(&ip, request.uri().path(), (app.clock)().timestamp_millis())
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
    fn for_path(path: &str) -> Self {
        if ["/sign-in", "/sign-up", "/change-password", "/change-email"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            Self {
                window_ms: 10_000,
                max: 3,
            }
        } else if [
            "/request-password-reset",
            "/send-verification-email",
            "/email-otp/send-verification-otp",
            "/email-otp/request-password-reset",
        ]
        .contains(&path)
            || path.starts_with("/forget-password")
        {
            Self {
                window_ms: 60_000,
                max: 3,
            }
        } else {
            Self {
                window_ms: 10_000,
                max: 100,
            }
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

#[cfg(test)]
mod tests {
    use super::*;
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
                    RateLimiter { enabled: true }
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
