//! Application-supplied validators for declared unversioned request schemas.
use forklaunch_contract::{RequestContract, RouteContract, ValidationMode};
use forklaunch_http::{FrameworkRequest, FrameworkResponse};
use std::{collections::BTreeMap, sync::Arc};

type Validator = dyn Fn(&FrameworkRequest, Option<&str>) -> Option<FrameworkResponse> + Send + Sync;

#[derive(Clone)]
struct Registration {
    schema: RequestContract,
    validate: Arc<Validator>,
}

/// Validators are bound to a declared request schema and method/operation pair.
/// Callbacks run after authentication and before either local or Axum handlers,
/// only in error-validation mode. They supplement native contract checks.
/// The optional raw query preserves repeated keys that `FrameworkRequest::query`
/// flattens. A returned response rejects the request; its diagnostics must be safe
/// for public consumption. This API does not translate validator semantics.
#[derive(Clone, Default)]
pub struct RequestValidators {
    entries: BTreeMap<String, Registration>,
}

impl RequestValidators {
    /// Register a validator for an unversioned declared request schema.
    /// # Errors
    /// Rejects versioned routes and duplicate method/operation registrations.
    pub fn insert(
        &mut self,
        route: &RouteContract,
        validate: impl Fn(&FrameworkRequest, Option<&str>) -> Option<FrameworkResponse>
        + Send
        + Sync
        + 'static,
    ) -> Result<(), String> {
        if !route.versions.is_empty() {
            return Err("Request validators require an unversioned route".into());
        }
        let key = key(route);
        if self.entries.contains_key(&key) {
            return Err("Duplicate request validator operation".into());
        }
        self.entries.insert(
            key,
            Registration {
                schema: route.request.clone(),
                validate: Arc::new(validate),
            },
        );
        Ok(())
    }

    pub(super) fn validate_routes(&self, routes: &[RouteContract]) -> Result<(), String> {
        for (key, registration) in &self.entries {
            let matches: Vec<_> = routes
                .iter()
                .filter(|route| self::key(route) == *key)
                .collect();
            if matches.len() != 1 {
                return Err("Request validator must identify exactly one route".into());
            }
            let route = matches[0];
            if !route.versions.is_empty() || route.request != registration.schema {
                return Err("Request validator schema no longer matches its route".into());
            }
        }
        Ok(())
    }

    pub(super) fn extend(&mut self, other: Self) {
        for (key, entry) in other.entries {
            assert!(
                self.entries.insert(key, entry).is_none(),
                "duplicate request validator operation"
            );
        }
    }

    pub(super) fn reject(
        &self,
        route: &RouteContract,
        request: &FrameworkRequest,
        raw_query: Option<&str>,
    ) -> Option<FrameworkResponse> {
        if route.options.request_validation != ValidationMode::Error {
            return None;
        }
        self.entries
            .get(&key(route))
            .and_then(|entry| (entry.validate)(request, raw_query))
    }
}

fn key(route: &RouteContract) -> String {
    format!("{} {}", route.method.as_str(), route.operation_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ForkLaunchRouter, RouterConfig};
    use forklaunch_contract::FieldContract;
    use forklaunch_core::ServiceInfo;
    use forklaunch_http::LocalRouter;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    #[test]
    fn registrations_fail_closed_on_stale_or_ambiguous_schemas() {
        let route = RouteContract::get("Query", "Query", "/query")
            .with_query(FieldContract::optional("limit", "Integer"));
        let mut validators = RequestValidators::default();
        validators.insert(&route, |_, _| None).unwrap();
        assert!(validators.insert(&route, |_, _| None).is_err());
        assert!(validators.validate_routes(&[]).is_err());
        assert!(
            validators
                .validate_routes(&[route.clone(), route.clone()])
                .is_err()
        );
        let changed = route
            .clone()
            .with_query(FieldContract::optional("other", "String"));
        assert!(validators.validate_routes(&[changed]).is_err());
        assert!(validators.validate_routes(&[route]).is_ok());
    }

    #[tokio::test]
    async fn callbacks_run_once_before_local_and_axum_handlers_after_nesting() {
        for local in [false, true] {
            for mode in [
                ValidationMode::Error,
                ValidationMode::Warning,
                ValidationMode::None,
            ] {
                check_route(local, mode, false).await;
            }
            check_route(local, ValidationMode::Error, true).await;
        }
    }

    async fn check_route(local: bool, mode: ValidationMode, unauthorized: bool) {
        let validations = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut contract = RouteContract::get("Query", "Query", "/query")
            .with_query(FieldContract::optional("limit", "Integer"))
            .with_response(200, None);
        contract.options.request_validation = mode;
        if unauthorized {
            contract.access = forklaunch_contract::AccessLevel::Authenticated;
        }
        let mut validators = RequestValidators::default();
        let count = validations.clone();
        validators
            .insert(&contract, move |_, raw| {
                count.fetch_add(1, Ordering::SeqCst);
                (raw == Some("limit=bad&limit=2"))
                    .then(|| FrameworkResponse::text(400, "invalid limit"))
            })
            .unwrap();
        let count = calls.clone();
        let child = if local {
            let mut handlers = LocalRouter::new();
            handlers.register(contract.clone(), move |_: FrameworkRequest| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(FrameworkResponse::text(200, "ok"))
            });
            ForkLaunchRouter::from_local_router(
                ServiceInfo::new("child", "1"),
                [contract],
                handlers,
            )
        } else {
            ForkLaunchRouter::new(ServiceInfo::new("child", "1")).get(contract, move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    "ok"
                }
            })
        }
        .with_config(RouterConfig {
            request_validators: validators,
            ..RouterConfig::default()
        });
        let router = ForkLaunchRouter::new(ServiceInfo::new("parent", "1"))
            .nest("/nested", child)
            .into_router();
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .uri("/nested/query?limit=bad&limit=2")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let rejecting = mode == ValidationMode::Error && !unauthorized;
        assert_eq!(
            response.status().as_u16(),
            if unauthorized {
                401
            } else if rejecting {
                400
            } else {
                200
            }
        );
        assert_eq!(validations.load(Ordering::SeqCst), usize::from(rejecting));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(!rejecting && !unauthorized)
        );
    }
}
