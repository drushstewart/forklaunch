//! Trusted post-persistence invitation delivery with bounded configured fields.
use crate::background::DeliveryFuture;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use zeroize::Zeroizing;

/// Owned sensitive callback data; deliberately without Debug/Serde.
/// No session credentials, raw request, or arbitrary additional fields are exposed.
/// Strings in the configured projections are bounded to 4096 UTF-8 bytes each.
/// Metadata retains its stored JSON shape within the documented resource limits.
pub struct InvitationDeliveryContext {
    id: Zeroizing<String>,
    role: Zeroizing<String>,
    email: Zeroizing<String>,
    organization: Value,
    invitation: Value,
    inviter: Value,
}
impl InvitationDeliveryContext {
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }
    #[must_use]
    pub fn organization(&self) -> &Value {
        &self.organization
    }
    #[must_use]
    pub fn invitation(&self) -> &Value {
        &self.invitation
    }
    /// Member projection with a nested configured public `user` projection.
    #[must_use]
    pub fn inviter(&self) -> &Value {
        &self.inviter
    }
}
/// Application-owned invitation email delivery. Preparation runs after persistence;
/// the returned future is awaited by default or handed to the shared dispatcher.
/// Rust futures are lazy; blocking code and panics are outside this Result contract.
pub trait InvitationEmailDelivery: Send + Sync {
    /// # Errors
    /// Preparation errors become sanitized empty HTTP500 responses after persistence.
    /// Future and scheduler errors are sanitized diagnostics and do not fail admission.
    fn prepare(&self, context: InvitationDeliveryContext) -> BlueprintResult<DeliveryFuture>;
}
pub(super) fn failure() -> BlueprintError {
    BlueprintError::new(
        "INVITATION_DELIVERY_PREPARATION_FAILED",
        "Unable to prepare invitation delivery",
    )
}
pub(super) fn projection(value: &Value, fields: &[&str]) -> BlueprintResult<Value> {
    // Validate borrowed selected values before cloning: no recursive clone of malformed input.
    for field in fields {
        if let Some(v) = value.get(*field)
            && !(v.is_null() || v.as_str().is_some_and(|s| s.len() <= 4096))
        {
            return Err(failure());
        }
    }
    let mut output = serde_json::Map::new();
    for field in fields {
        if let Some(v) = value.get(*field) {
            output.insert((*field).to_owned(), v.clone());
        }
    }
    Ok(Value::Object(output))
}
/// Metadata is an adapter-owned JSON value: do not parse strings or coerce scalars.
/// Check borrowed data before cloning (64 levels, 4096 nodes, 4096 total key/string bytes).
pub(super) fn organization_projection(value: &Value) -> BlueprintResult<Value> {
    let mut output = projection(
        value,
        &[
            "id",
            "name",
            "slug",
            "logo",
            "createdAt",
            "domain",
            "subscription",
            "status",
        ],
    )?;
    if let Some(metadata) = value.get("metadata") {
        metadata_budget(metadata, 0, &mut 4096, &mut 4096)?;
        output["metadata"] = metadata.clone();
    }
    Ok(output)
}
fn metadata_budget(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
    bytes: &mut usize,
) -> BlueprintResult<()> {
    if depth > 64 {
        return Err(failure());
    }
    *nodes = nodes.checked_sub(1).ok_or_else(failure)?;
    match value {
        Value::String(text) => *bytes = bytes.checked_sub(text.len()).ok_or_else(failure)?,
        Value::Array(values) => {
            for value in values {
                metadata_budget(value, depth + 1, nodes, bytes)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                *bytes = bytes.checked_sub(key.len()).ok_or_else(failure)?;
                metadata_budget(value, depth + 1, nodes, bytes)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
    Ok(())
}
pub(super) fn inviter(member: &Value, user: &Value) -> BlueprintResult<Value> {
    let mut member = projection(
        member,
        &["id", "organizationId", "userId", "role", "createdAt"],
    )?;
    member["user"] = crate::accounts::reset_request::projection(user).map_err(|_| failure())?;
    Ok(member)
}
pub(super) fn context(
    inviter: Value,
    organization: &Value,
    invitation: &Value,
) -> BlueprintResult<InvitationDeliveryContext> {
    let organization = organization_projection(organization)?;
    let invitation = projection(
        invitation,
        &[
            "id",
            "email",
            "role",
            "organizationId",
            "inviterId",
            "status",
            "expiresAt",
            "createdAt",
            "teamId",
        ],
    )?;
    let get = |field| {
        invitation
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(failure)
    };
    Ok(InvitationDeliveryContext {
        id: Zeroizing::new(get("id")?),
        role: Zeroizing::new(get("role")?),
        email: Zeroizing::new(get("email")?.to_lowercase()),
        organization,
        invitation,
        inviter,
    })
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn metadata_resource_limits_precede_clone_and_preserve_other_projections() {
        assert!(organization_projection(&json!({"metadata":"x".repeat(4096)})).is_ok());
        assert!(organization_projection(&json!({"metadata":"x".repeat(4097)})).is_err());
        assert!(organization_projection(&json!({"metadata":vec![Value::Null;4095]})).is_ok());
        assert!(organization_projection(&json!({"metadata":vec![Value::Null;4096]})).is_err());
        let mut nested = Value::Null;
        for _ in 0..64 {
            nested = json!([nested]);
        }
        assert!(organization_projection(&json!({"metadata":nested})).is_ok());
        assert!(organization_projection(&json!({"metadata":[nested]})).is_err());
        assert!(organization_projection(&json!({"metadata":{"long":"x".repeat(4093)}})).is_err());
        assert!(organization_projection(&json!({"name":42,"metadata":{}})).is_err());
        assert_eq!(
            organization_projection(&json!({"id":"owned","extra":"omitted"})).unwrap(),
            json!({"id":"owned"})
        );
    }
}
