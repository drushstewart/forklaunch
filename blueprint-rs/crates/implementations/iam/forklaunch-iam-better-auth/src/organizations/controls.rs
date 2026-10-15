//! Trusted organization deletion and dynamic-role admission controls.
/// Deletion remains enabled and dynamic roles unlimited by default. The optional
/// exact nonnegative integer role quota counts all organization-role records,
/// including configured-name rows, under the native store's writer lock.
/// Zero denies new role creation. This does not alter predefined-name checks or
/// grant creators permission to create roles; policy callbacks are separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrganizationControls {
    pub(crate) disable_deletion: bool,
    pub(super) maximum_roles: Option<u32>,
}
impl OrganizationControls {
    #[must_use]
    pub const fn new(disable_deletion: bool, maximum_roles: Option<u32>) -> Self {
        Self {
            disable_deletion,
            maximum_roles,
        }
    }
    pub(crate) fn check_deletion(self) -> forklaunch_blueprint_core::BlueprintResult<()> {
        if self.disable_deletion {
            Err(forklaunch_blueprint_core::BlueprintError::new(
                "ORGANIZATION_DELETION_DISABLED",
                "Organization deletion is disabled",
            ))
        } else {
            Ok(())
        }
    }
}
