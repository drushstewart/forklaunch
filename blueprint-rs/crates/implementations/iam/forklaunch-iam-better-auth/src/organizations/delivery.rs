//! Trusted post-persistence invitation delivery with bounded configured fields.
use crate::background::DeliveryFuture;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use zeroize::Zeroizing;

/// Owned sensitive callback data; deliberately without Debug/Serde.
/// No session credentials, raw request, or arbitrary additional fields are exposed.
/// Strings in the configured projections are bounded to 4096 UTF-8 bytes each.
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
fn projection(value: &Value, fields: &[&str]) -> BlueprintResult<Value> {
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
    let organization = projection(
        organization,
        &[
            "id",
            "name",
            "slug",
            "logo",
            "createdAt",
            "metadata",
            "domain",
            "subscription",
            "status",
        ],
    )?;
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
