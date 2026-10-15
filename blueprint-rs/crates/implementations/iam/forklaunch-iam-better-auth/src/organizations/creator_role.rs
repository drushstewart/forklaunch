//! Trusted creator identity policy, independent of declared permission roles.
use super::{Arc, BlueprintError, BlueprintResult};
/// Exact configured creator role. Empty input uses the upstream `owner` fallback.
/// This does not declare a role or grant general organization permissions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatorRole(Arc<str>);
impl CreatorRole {
    /// Accepts at most 4096 UTF-8 bytes without trimming or splitting the value.
    /// # Errors
    /// Returns a configuration error if the role exceeds the supported bound.
    pub fn new(role: &str) -> BlueprintResult<Self> {
        if role.len() > 4096 {
            return Err(BlueprintError::new(
                "INVALID_CREATOR_ROLE",
                "Creator role exceeds supported bound",
            ));
        }
        Ok(Self(Arc::from(if role.is_empty() {
            "owner"
        } else {
            role
        })))
    }
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for CreatorRole {
    fn default() -> Self {
        Self(Arc::from("owner"))
    }
}
