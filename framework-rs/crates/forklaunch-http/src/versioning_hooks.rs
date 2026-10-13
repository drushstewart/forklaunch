//! Native body preprocessing/transformation around per-version schema checks.
use super::{BodyParserHooks, VersionFieldHooks, VersionedRoutePlan};
use crate::ContractValidationLocation;
use forklaunch_contract::RouteContract;
use serde_json::Value;
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

type Transform = dyn Fn(Option<&Value>) -> Result<Option<Value>, String> + Send + Sync;

/// Runtime functions are deliberately separate from exported schema metadata.
#[derive(Clone, Default)]
pub struct VersionBodyHooks {
    preprocess: Option<Arc<Transform>>,
    transform: Option<Arc<Transform>>,
}
impl std::fmt::Debug for VersionBodyHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VersionBodyHooks")
            .field("preprocess", &self.preprocess.is_some())
            .field("transform", &self.transform.is_some())
            .finish()
    }
}
impl VersionBodyHooks {
    /// Run before this version's schema checks. Errors reject only this version.
    #[must_use]
    pub fn with_preprocess(
        mut self,
        callback: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        self.preprocess = Some(Arc::new(move |value| value.map(&callback).transpose()));
        self
    }
    /// Run after this version's schema checks. Errors reject only this version.
    #[must_use]
    pub fn with_transform(
        mut self,
        callback: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    ) -> Self {
        self.transform = Some(Arc::new(move |value| value.map(&callback).transpose()));
        self
    }
    /// Run before schema checks, including when the parsed value is absent.
    /// `None` represents absence; `Some(Value::Null)` is an explicit JSON null.
    /// Return a value to supply a missing-value default. Returning `None` still
    /// requires the selected contract to accept absence. Replaces any previously
    /// registered preprocessor, including one set with `with_preprocess`.
    #[must_use]
    pub fn with_optional_preprocess(
        mut self,
        callback: impl Fn(Option<&Value>) -> Result<Option<Value>, String> + Send + Sync + 'static,
    ) -> Self {
        self.preprocess = Some(Arc::new(callback));
        self
    }
    /// Run after successful schema checks, allowing the transformed output to
    /// become absent. Errors reject only this version. Replaces any previously
    /// registered transform, including one set with `with_transform`.
    #[must_use]
    pub fn with_optional_transform(
        mut self,
        callback: impl Fn(Option<&Value>) -> Result<Option<Value>, String> + Send + Sync + 'static,
    ) -> Self {
        self.transform = Some(Arc::new(callback));
        self
    }
    pub(super) fn preprocess<'a>(
        &self,
        body: Option<&'a Value>,
    ) -> Result<Option<Cow<'a, Value>>, String> {
        match &self.preprocess {
            Some(callback) => callback(body).map(|value| value.map(Cow::Owned)),
            None => Ok(body.map(Cow::Borrowed)),
        }
    }
    pub(super) fn transform<'a>(
        &self,
        body: Option<Cow<'a, Value>>,
    ) -> Result<Option<Cow<'a, Value>>, String> {
        match &self.transform {
            Some(callback) => callback(body.as_deref()).map(|value| value.map(Cow::Owned)),
            None => Ok(body),
        }
    }
}

#[derive(Clone, Debug)]
struct FieldHooks {
    version: String,
    location: ContractValidationLocation,
    name: String,
    hooks: VersionFieldHooks,
}

/// Hooks are indexed by HTTP method and operation name, independent of mount prefixes.
#[derive(Clone, Default, Debug)]
pub struct VersionBodyHooksRegistry {
    routes: BTreeMap<String, BTreeMap<String, VersionBodyHooks>>,
    parsers: BTreeMap<String, BodyParserHooks>,
    fields: BTreeMap<String, Vec<FieldHooks>>,
}
impl VersionBodyHooksRegistry {
    /// # Errors
    /// Rejects unknown versions and versions with no declared body.
    pub fn insert(
        &mut self,
        route: &RouteContract,
        version: &str,
        hooks: VersionBodyHooks,
    ) -> Result<(), String> {
        let projected = route.for_version(version).ok_or("Unknown hook version")?;
        if projected.request.body.is_none() {
            return Err("Body hooks require a declared body".into());
        }
        self.routes
            .entry(route_key(route))
            .or_default()
            .insert(version.into(), hooks);
        Ok(())
    }
    /// Register callbacks for one declared parameter, query or header field.
    /// # Errors
    /// Rejects invalid contracts, unknown versions or undeclared fields.
    pub fn insert_field(
        &mut self,
        route: &RouteContract,
        version: &str,
        location: ContractValidationLocation,
        name: &str,
        hooks: VersionFieldHooks,
    ) -> Result<(), String> {
        VersionedRoutePlan::compile(route)?.with_field_hooks(
            version,
            location,
            name,
            hooks.clone(),
        )?;
        let name = if location == ContractValidationLocation::Header {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        let fields = self.fields.entry(route_key(route)).or_default();
        fields.retain(|field| {
            field.version != version || field.location != location || field.name != name
        });
        fields.push(FieldHooks {
            version: version.into(),
            location,
            name,
            hooks,
        });
        Ok(())
    }
    /// # Errors
    /// Rejects routes without a supported shared body parser.
    pub fn insert_parser(
        &mut self,
        route: &RouteContract,
        hooks: BodyParserHooks,
    ) -> Result<(), String> {
        VersionedRoutePlan::compile(route)?.with_parser_hooks(hooks.clone())?;
        self.parsers.insert(route_key(route), hooks);
        Ok(())
    }
    /// Check that each registered operation identifies exactly one route.
    /// # Errors
    /// Rejects unused hooks and ambiguous method/operation pairs.
    pub fn validate_routes(&self, routes: &[RouteContract]) -> Result<(), String> {
        let mut counts = BTreeMap::<String, usize>::new();
        for route in routes {
            *counts.entry(route_key(route)).or_default() += 1;
        }
        for key in self
            .routes
            .keys()
            .chain(self.parsers.keys())
            .chain(self.fields.keys())
        {
            if counts.get(key) != Some(&1) {
                return Err(format!(
                    "Version hook operation must identify exactly one route: {key}"
                ));
            }
        }
        Ok(())
    }
    /// Combine hook registries when mounting routers.
    /// # Panics
    /// Panics if both registries define hooks for the same method and operation.
    pub fn extend(&mut self, other: Self) {
        for (key, hooks) in other.fields {
            assert!(
                self.fields.insert(key, hooks).is_none(),
                "duplicate field hook operation"
            );
        }
        for (key, hooks) in other.parsers {
            assert!(
                self.parsers.insert(key, hooks).is_none(),
                "duplicate parser hook operation"
            );
        }
        for (key, hooks) in other.routes {
            assert!(
                self.routes.insert(key, hooks).is_none(),
                "duplicate body hook operation"
            );
        }
    }
    /// # Errors
    /// Rejects invalid schemas or hooks referring to removed versions.
    pub fn compile(&self, route: &RouteContract) -> Result<VersionedRoutePlan, String> {
        let mut plan = VersionedRoutePlan::compile(route)?;
        if let Some(versions) = self.routes.get(&route_key(route)) {
            for (version, hooks) in versions {
                plan = plan.with_body_hooks(version, hooks.clone())?;
            }
        }
        if let Some(fields) = self.fields.get(&route_key(route)) {
            for field in fields {
                plan = plan.with_field_hooks(
                    &field.version,
                    field.location,
                    &field.name,
                    field.hooks.clone(),
                )?;
            }
        }
        if let Some(hooks) = self.parsers.get(&route_key(route)) {
            plan = plan.with_parser_hooks(hooks.clone())?;
        }
        Ok(plan)
    }
}
fn route_key(route: &RouteContract) -> String {
    format!("{} {}", route.method.as_str(), route.operation_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, FrameworkRequest, HttpMethod, RequestContext};
    use forklaunch_contract::{BodyContract, RequestContract, VersionedContract};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn hooks_reject_invalid_registration_and_transform_only_valid_inputs() {
        let mut route = RouteContract::post("Hook", "Hook", "/hook");
        route.options.body_parser.json_strict = false;
        route = route.with_version(VersionedContract {
            version: "one".into(),
            request: RequestContract::default().with_body(BodyContract {
                schema: forklaunch_contract::SchemaRef::named("Input")
                    .with_json_schema(r#"{"type":"integer"}"#),
                ..BodyContract::json("Input")
            }),
            responses: vec![],
        });
        let mut registry = VersionBodyHooksRegistry::default();
        assert!(
            registry
                .insert(&route, "missing", VersionBodyHooks::default())
                .is_err()
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        registry
            .insert(
                &route,
                "one",
                VersionBodyHooks::default().with_transform(move |value| {
                    count.fetch_add(1, Ordering::SeqCst);
                    Ok(value.clone())
                }),
            )
            .unwrap();
        assert!(registry.validate_routes(&[]).is_err());
        assert!(
            registry
                .validate_routes(&[route.clone(), route.clone()])
                .is_err()
        );
        assert!(
            registry
                .validate_routes(std::slice::from_ref(&route))
                .is_ok()
        );
        let plan = registry.compile(&route).unwrap();
        let request = FrameworkRequest::new(HttpMethod::Post, "/hook", RequestContext::new("hook"))
            .with_header("content-type", "application/json")
            .with_body(Body::Text("false".into()));
        assert!(plan.select(&request).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        route.versions.get_mut("one").unwrap().request.body = None;
        assert!(registry.compile(&route).is_err());
        assert!(
            registry
                .insert(&route, "one", VersionBodyHooks::default())
                .is_err()
        );
    }
}
