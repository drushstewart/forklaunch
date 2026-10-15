//! Trusted synchronous role overrides, bound to an immutable declarative policy.
use super::{
    Arc, BlueprintError, BlueprintResult, OrganizationAccessControl, OrganizationStatements, Value,
};
use std::{collections::BTreeMap, ops::Deref};

/// Borrowed immutable counterpart of a role's statements and authorization request.
/// No raw JavaScript receiver, user/session data or mutable request is exposed.
pub struct OrganizationAuthorizationRequest<'a> {
    role: &'a str,
    statements: &'a OrganizationStatements,
    permissions: &'a serde_json::Map<String, Value>,
}
impl OrganizationAuthorizationRequest<'_> {
    /// Exact selected static role name.
    #[must_use]
    pub fn role(&self) -> &str {
        self.role
    }
    /// The role's declarative statements, before any dynamic-row replacement.
    #[must_use]
    pub fn statements(&self) -> &OrganizationStatements {
        self.statements
    }
    /// Admitted request fields; iteration through `entries` follows JS own-key order.
    #[must_use]
    pub fn permissions(&self) -> &serde_json::Map<String, Value> {
        self.permissions
    }
    /// Numeric index resources precede other resources in their insertion order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &Value)> {
        super::permission::own_entries(self.permissions)
            .into_iter()
            .map(|(key, value)| (key.as_str(), value))
    }
}

/// Synchronous trusted equivalent of a declared `Role.authorize` implementation.
/// It runs inline during authoritative permission admission. Applications own any
/// callback work; no async scheduling, raw JS mutation or panic isolation is promised.
pub trait OrganizationRoleAuthorizer: Send + Sync {
    /// # Errors
    /// Failure becomes a sanitized empty HTTP 500; error details are not published.
    fn authorize(&self, request: OrganizationAuthorizationRequest<'_>) -> BlueprintResult<bool>;
}
impl<F> OrganizationRoleAuthorizer for F
where
    F: for<'a> Fn(OrganizationAuthorizationRequest<'a>) -> BlueprintResult<bool> + Send + Sync,
{
    fn authorize(&self, request: OrganizationAuthorizationRequest<'_>) -> BlueprintResult<bool> {
        self(request)
    }
}

/// Validated own-role callbacks and their exact declarative policy snapshot.
/// Cloning shares callback instances. There is intentionally no `Debug` or serialization implementation.
#[derive(Clone)]
pub struct OrganizationRoleAuthorizers {
    control: OrganizationAccessControl,
    callbacks: Arc<BTreeMap<String, Arc<dyn OrganizationRoleAuthorizer>>>,
}
impl OrganizationRoleAuthorizers {
    /// Binds overrides only to exact own static names, including explicit prototype names.
    /// The access-control bounds also bound registry cardinality and key sizes.
    /// # Errors
    /// Rejects callback names absent from the supplied static map, before any I/O.
    pub fn new(
        control: OrganizationAccessControl,
        callbacks: BTreeMap<String, Arc<dyn OrganizationRoleAuthorizer>>,
    ) -> BlueprintResult<Self> {
        if callbacks.keys().any(|role| !control.predefined(role)) {
            return Err(BlueprintError::new(
                "INVALID_ORGANIZATION_ROLE_AUTHORIZERS",
                "Authorizers require declared static roles",
            ));
        }
        Ok(Self {
            control,
            callbacks: Arc::new(callbacks),
        })
    }
}

#[derive(Clone)]
pub(crate) struct Authorization {
    registry: OrganizationRoleAuthorizers,
    cache: super::OrganizationPermissionCache,
}
impl Deref for Authorization {
    type Target = OrganizationAccessControl;
    fn deref(&self) -> &Self::Target {
        &self.registry.control
    }
}
impl From<OrganizationAccessControl> for Authorization {
    fn from(control: OrganizationAccessControl) -> Self {
        Self {
            registry: OrganizationRoleAuthorizers {
                control,
                callbacks: Arc::new(BTreeMap::new()),
            },
            cache: super::OrganizationPermissionCache::default(),
        }
    }
}
impl From<OrganizationRoleAuthorizers> for Authorization {
    fn from(registry: OrganizationRoleAuthorizers) -> Self {
        Self {
            registry,
            cache: super::OrganizationPermissionCache::default(),
        }
    }
}
impl Authorization {
    pub(crate) fn with_cache(mut self, cache: super::OrganizationPermissionCache) -> Self {
        self.cache = cache;
        self
    }
    pub(crate) fn cache(&self) -> &super::OrganizationPermissionCache {
        &self.cache
    }
    pub(super) fn custom(
        &self,
        role: &str,
        permissions: &serde_json::Map<String, Value>,
    ) -> Option<BlueprintResult<bool>> {
        self.registry.callbacks.get(role).map(|callback| {
            // The pinned record parser drops this key before invoking the role.
            // Project only when an override is actually invoked; default paths
            // retain their existing borrowed, no-clone authorization behavior.
            let admitted;
            let permissions = if permissions.contains_key("__proto__") {
                admitted = permissions
                    .iter()
                    .filter(|(key, _)| key.as_str() != "__proto__")
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                &admitted
            } else {
                permissions
            };
            let statements = self
                .registry
                .control
                .role_statements(role)
                .ok_or_else(failure)?;
            callback
                .authorize(OrganizationAuthorizationRequest {
                    role,
                    statements,
                    permissions,
                })
                .map_err(|_| failure())
        })
    }
}

fn failure() -> BlueprintError {
    BlueprintError::new("HTTP_INTERNAL_EMPTY", "Organization authorization failed")
}
