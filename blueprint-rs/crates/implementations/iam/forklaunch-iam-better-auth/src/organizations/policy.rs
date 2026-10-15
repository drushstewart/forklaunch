//! Trusted scalar organization-plugin options; never populated from request JSON.
/// Bounded nonnegative integer subset of Better Auth's scalar organization options.
///
/// Creation is allowed and unlimited by default; membership defaults to 100.
/// `organization_limit: Some(0)` denies creation, while a zero membership limit
/// falls back to 100. Organization limits use the upstream default adapter's
/// first 100 matching membership records, including non-owner and duplicate rows;
/// consequently limits above 100 are not reached by that window. They are not
/// an unbounded quota. Membership counts retain the store's bounded scan contract.
/// Async policy callbacks and trusted server user-ID impersonation are separate APIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrganizationPolicy {
    pub(super) allow_create: bool,
    pub(super) organization_limit: Option<u32>,
    membership_limit: u32,
}
impl Default for OrganizationPolicy {
    fn default() -> Self {
        Self::new(true, None, 100)
    }
}
impl OrganizationPolicy {
    /// Installs trusted scalar options. Integers are exact; zero semantics differ
    /// deliberately between the two limits to match the upstream plugin.
    #[must_use]
    pub const fn new(
        allow_create: bool,
        organization_limit: Option<u32>,
        membership_limit: u32,
    ) -> Self {
        Self {
            allow_create,
            organization_limit,
            membership_limit: if membership_limit == 0 {
                100
            } else {
                membership_limit
            },
        }
    }
    pub(super) fn member_limit(self) -> usize {
        usize::try_from(self.membership_limit).unwrap_or(usize::MAX)
    }
}
