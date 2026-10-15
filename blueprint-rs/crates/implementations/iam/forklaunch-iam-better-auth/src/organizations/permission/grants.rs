//! Finite object-prototype effects of the pinned declarative role merger.
//! This models only Role objects constructed from admitted statement records.
use super::{BlueprintResult, Statements, empty_error, prototype_name};
use std::collections::{BTreeMap, BTreeSet};

type EffectiveStatements = BTreeMap<String, Actions>;
#[derive(Clone)]
pub(super) enum Actions {
    List(Vec<String>),
    // Object-spreading a statement action array produces string-valued indices.
    Text(String),
}
impl Actions {
    pub(super) fn includes(&self, action: &str) -> bool {
        match self {
            Self::List(values) => values.iter().any(|value| value == action),
            Self::Text(value) => !value.is_empty() && value.contains(action),
        }
    }
    fn iterable(self) -> Vec<String> {
        match self {
            Self::List(values) => values,
            Self::Text(value) => value
                .chars()
                .map(|character| character.to_string())
                .collect(),
        }
    }
}
pub(super) struct Grants {
    own: BTreeMap<String, EffectiveStatements>,
    prototype: Option<EffectiveStatements>,
    replaced: BTreeSet<String>,
}
impl Grants {
    pub(super) fn new(roles: BTreeMap<String, Statements>) -> Self {
        Self {
            own: roles
                .into_iter()
                .map(|(role, statements)| {
                    (
                        role,
                        statements
                            .into_iter()
                            .map(|(key, actions)| (key, Actions::List(actions)))
                            .collect(),
                    )
                })
                .collect(),
            prototype: None,
            replaced: BTreeSet::new(),
        }
    }
    pub(super) fn replaced(&self, role: &str) -> bool {
        self.replaced.contains(role)
    }
    pub(super) fn get(&self, role: &str) -> BlueprintResult<Option<&EffectiveStatements>> {
        if let Some(own) = self.own.get(role) {
            return Ok(Some(own));
        }
        if role == "__proto__" && self.prototype.is_some() {
            return Ok(self.prototype.as_ref());
        }
        if prototype_name(role)
            || (self.prototype.is_some() && matches!(role, "authorize" | "statements"))
        {
            return Err(empty_error());
        }
        Ok(None)
    }
    fn inherited_statements(&self, role: &str) -> EffectiveStatements {
        let Some(prototype) = &self.prototype else {
            return BTreeMap::new();
        };
        if role == "__proto__" {
            return prototype.clone();
        }
        if role == "statements" {
            // acRoles.statements is the Role's statements object. Reading its
            // .statements property and spreading that array yields index keys.
            if let Some(Actions::List(values)) = prototype.get("statements") {
                return values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), Actions::Text(value.clone())))
                    .collect();
            }
        }
        BTreeMap::new()
    }
    pub(super) fn merge(&mut self, role: &str, permission: Statements) -> BlueprintResult<()> {
        let mut current = self
            .own
            .get(role)
            .cloned()
            .unwrap_or_else(|| self.inherited_statements(role));
        for (resource, actions) in permission {
            if !current.contains_key(&resource) && prototype_name(&resource) {
                return Err(empty_error());
            }
            let mut merged = current
                .remove(&resource)
                .map(Actions::iterable)
                .unwrap_or_default();
            for action in actions {
                if !merged.contains(&action) {
                    merged.push(action);
                }
            }
            current.insert(resource, Actions::List(merged));
        }
        self.replaced.insert(role.to_owned());
        if role == "__proto__" && !self.own.contains_key(role) {
            self.prototype = Some(current);
        } else {
            self.own.insert(role.to_owned(), current);
        }
        Ok(())
    }
}
