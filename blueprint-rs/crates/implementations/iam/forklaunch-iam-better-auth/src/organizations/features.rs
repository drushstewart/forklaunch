//! Trusted plugin topology; configured defaults enable both optional families.
use super::{BlueprintError, BlueprintResult};
use serde_json::Value;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrganizationFeatures {
    pub(crate) teams: bool,
    pub(crate) dynamic_access_control: bool,
}
impl Default for OrganizationFeatures {
    fn default() -> Self {
        Self::new(true, true)
    }
}
impl OrganizationFeatures {
    #[must_use]
    pub const fn new(teams: bool, dynamic_access_control: bool) -> Self {
        Self {
            teams,
            dynamic_access_control,
        }
    }
    #[cfg(feature = "http")]
    pub(crate) fn route(self, path: &str) -> bool {
        (!TEAM_ROUTES.contains(&path) || self.teams)
            && (!ROLE_ROUTES.contains(&path) || self.dynamic_access_control)
    }
    pub(super) fn require_teams(self) -> BlueprintResult<()> {
        if self.teams { Ok(()) } else { Err(disabled()) }
    }
    pub(super) fn require_roles(self) -> BlueprintResult<()> {
        if self.dynamic_access_control {
            Ok(())
        } else {
            Err(disabled())
        }
    }
    pub(super) fn member_team(self, team: Option<&str>) -> BlueprintResult<()> {
        if !self.teams && team.is_some_and(|t| !t.is_empty()) {
            Err(BlueprintError::new(
                "HTTP_MESSAGE_ONLY",
                "Teams are not enabled",
            ))
        } else {
            Ok(())
        }
    }
    pub(crate) fn session(self, value: &mut Value) {
        if !self.teams {
            remove(value, "activeTeamId");
        }
    }
    pub(super) fn invitation(self, value: &mut Value) {
        if !self.teams {
            remove(value, "teamId");
        }
    }
    pub(crate) fn view(
        self,
        mut view: crate::sessions::SessionView,
    ) -> crate::sessions::SessionView {
        self.session(&mut view.session);
        view
    }
}
fn remove(value: &mut Value, key: &str) {
    if let Some(value) = value.as_object_mut() {
        value.remove(key);
    }
}
fn disabled() -> BlueprintError {
    BlueprintError::new(
        "ORGANIZATION_FEATURE_DISABLED",
        "Organization plugin feature is disabled",
    )
}
#[cfg(feature = "http")]
pub(crate) const TEAM_ROUTES: &[&str] = &[
    "/organization/list-teams",
    "/organization/list-user-teams",
    "/organization/list-team-members",
    "/organization/set-active-team",
    "/organization/create-team",
    "/organization/update-team",
    "/organization/remove-team",
    "/organization/add-team-member",
    "/organization/remove-team-member",
];
#[cfg(feature = "http")]
pub(crate) const ROLE_ROUTES: &[&str] = &[
    "/organization/create-role",
    "/organization/update-role",
    "/organization/delete-role",
    "/organization/list-roles",
    "/organization/get-role",
];
