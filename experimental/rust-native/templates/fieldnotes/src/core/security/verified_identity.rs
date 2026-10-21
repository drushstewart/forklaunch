//! Nonforgeable workspace identity and application write admission.
use crate::{Role, SecurityError};
use uuid::Uuid;

/// Only constructed after the configured authentication service validates cookie and membership.
/// No Debug/Serialize: this object carries trusted identity, not client request data.
///
/// A caller cannot forge a scope from request JSON:
/// ```compile_fail
/// use fieldnotes_security::VerifiedTenant;
/// let forged: VerifiedTenant = serde_json::from_str(r#"{"user_id":"alice"}"#).unwrap();
/// ```
pub struct VerifiedTenant {
    user_id: String,
    tenant_id: Uuid,
    role: Role,
}
impl VerifiedTenant {
    pub(crate) fn from_verified_parts(user_id: String, tenant_id: Uuid, role: Role) -> Self {
        Self {
            user_id,
            tenant_id,
            role,
        }
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }
    pub fn tenant_id(&self) -> Uuid {
        self.tenant_id
    }
    pub fn role(&self) -> Role {
        self.role
    }
    pub fn require_write(&self) -> Result<(), SecurityError> {
        if self.role == Role::Viewer {
            Err(SecurityError::Forbidden)
        } else {
            Ok(())
        }
    }
}
