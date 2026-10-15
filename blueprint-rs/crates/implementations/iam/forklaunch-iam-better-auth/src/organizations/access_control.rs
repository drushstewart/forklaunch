//! Bounded declarative access control; no application callbacks or request policy.
use super::{Arc, BlueprintError, BlueprintResult};
use std::collections::BTreeMap;
/// Resource names and their declared action strings.
pub type OrganizationStatements = BTreeMap<String, Vec<String>>;
/// Static role names and their statements. An explicit map replaces the preset.
pub type OrganizationRoles = BTreeMap<String, OrganizationStatements>;
#[derive(Debug, PartialEq, Eq)]
struct Configuration {
    statements: OrganizationStatements,
    roles: OrganizationRoles,
}
/// Trusted immutable declarative equivalent of `ac.statements` and `roles`.
/// Static role statements need not be a subset of the resource catalog, matching
/// upstream runtime `newRole`; the catalog gates dynamic-role resource creation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganizationAccessControl(Arc<Configuration>);
impl OrganizationAccessControl {
    /// Keeps exact names and actions, including empty maps and strings.
    /// Limits: 128 roles, 128 resources per map, 4096 total entries/actions,
    /// 4096 UTF-8 bytes per string and 1 MiB of aggregate string data.
    /// # Errors
    /// Rejects configurations outside this bounded typed profile before any I/O.
    pub fn new(
        statements: OrganizationStatements,
        roles: OrganizationRoles,
    ) -> BlueprintResult<Self> {
        let mut budget = Budget::default();
        if roles.len() > 128 {
            return Err(invalid());
        }
        budget.map(&statements)?;
        for (name, statements) in &roles {
            budget.string(name)?;
            budget.map(statements)?;
        }
        Ok(Self(Arc::new(Configuration { statements, roles })))
    }
    pub(super) fn roles(&self) -> OrganizationRoles {
        self.0.roles.clone()
    }
    pub(super) fn predefined(&self, name: &str) -> bool {
        self.0.roles.contains_key(name)
    }
    pub(super) fn admitted_role(&self, name: &str) -> bool {
        matches!(name, "owner" | "admin" | "member") || self.predefined(name)
    }
    pub(super) fn resource(&self, name: &str) -> bool {
        self.0.statements.contains_key(name)
    }
}
impl Default for OrganizationAccessControl {
    fn default() -> Self {
        let statements = BTreeMap::from([(
            "platform".into(),
            vec!["platform:read".into(), "platform:write".into()],
        )]);
        let roles = ["owner", "admin", "editor", "system", "viewer"]
            .into_iter()
            .map(|name| {
                let actions = if name == "viewer" {
                    vec!["platform:read".into()]
                } else {
                    vec!["platform:read".into(), "platform:write".into()]
                };
                (name.into(), BTreeMap::from([("platform".into(), actions)]))
            })
            .collect();
        Self(Arc::new(Configuration { statements, roles }))
    }
}
#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: usize,
}
impl Budget {
    fn string(&mut self, value: &str) -> BlueprintResult<()> {
        self.entries += 1;
        self.bytes += value.len();
        if value.len() > 4096 || self.entries > 4096 || self.bytes > 1_048_576 {
            return Err(invalid());
        }
        Ok(())
    }
    fn map(&mut self, statements: &OrganizationStatements) -> BlueprintResult<()> {
        if statements.len() > 128 {
            return Err(invalid());
        }
        for (resource, actions) in statements {
            self.string(resource)?;
            for action in actions {
                self.string(action)?;
            }
        }
        Ok(())
    }
}
fn invalid() -> BlueprintError {
    BlueprintError::new(
        "INVALID_ORGANIZATION_ACCESS_CONTROL",
        "Organization access control exceeds supported bounds",
    )
}
