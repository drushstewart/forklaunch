//! Trusted scalar team options; request bodies cannot change admission policy.
/// Bounded nonnegative integer subset of the configured organization plugin.
/// Defaults create a team with each organization, impose no team/member maximum,
/// and prevent deleting the last team. A zero team maximum is unlimited; a zero
/// member maximum denies new memberships but preserves existing pairs.
/// Team counts and invitation member hydration retain the adapter's first-100
/// windows. Seat admission uses the actual bounded count and retained overcount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamPolicy {
    pub(super) default_team: bool,
    pub(super) maximum_teams: Option<u32>,
    pub(super) maximum_members: Option<u32>,
    pub(super) allow_removing_all: bool,
}
impl Default for TeamPolicy {
    fn default() -> Self {
        Self::new(true, None, None, false)
    }
}
impl TeamPolicy {
    #[must_use]
    pub const fn new(
        default_team: bool,
        maximum_teams: Option<u32>,
        maximum_members: Option<u32>,
        allow_removing_all: bool,
    ) -> Self {
        Self {
            default_team,
            maximum_teams,
            maximum_members,
            allow_removing_all,
        }
    }
}
pub(super) fn member_limit() -> forklaunch_blueprint_core::BlueprintError {
    forklaunch_blueprint_core::BlueprintError::new(
        "TEAM_MEMBER_LIMIT_REACHED",
        "Team member limit reached",
    )
}
