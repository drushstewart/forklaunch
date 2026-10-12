//! Configured team projections, membership and session-selection boundaries.
use super::mutation::find_member;
use super::permission::allowed;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationSelection, OrganizationService, Utc, Value, json, load, member, text, timestamp,
    unauthorized,
};
mod mutation;
mod read;
mod selection;
mod users;

fn no_active() -> BlueprintError {
    BlueprintError::new("NO_ACTIVE_ORGANIZATION", "No active organization")
}
fn missing() -> BlueprintError {
    BlueprintError::new("TEAM_NOT_FOUND", "Team not found")
}
fn no_team_member() -> BlueprintError {
    BlueprintError::new(
        "USER_IS_NOT_A_MEMBER_OF_THE_TEAM",
        "User is not a member of the team",
    )
}
fn nonmember() -> BlueprintError {
    BlueprintError::new(
        "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION",
        "You are not a member of this organization",
    )
}
fn public_team(mut row: Value) -> Value {
    if let Some(row) = row.as_object_mut() {
        row.remove("memberCount");
    }
    row
}
fn public_member(mut row: Value) -> Value {
    if let Some(row) = row.as_object_mut() {
        row.remove("membershipKey");
    }
    row
}
async fn team_member<T: AuthTransaction>(
    tx: &mut T,
    team: &str,
    user: &str,
) -> BlueprintResult<bool> {
    Ok(tx
        .list(Model::TeamMember)
        .await?
        .iter()
        .any(|row| row["teamId"] == team && row["userId"] == user))
}
async fn joined_member<T: AuthTransaction>(
    tx: &mut T,
    user: &str,
    org: &str,
) -> BlueprintResult<Option<Value>> {
    if tx.get(Model::User, user).await?.is_none() {
        return Ok(None);
    }
    find_member(tx, user, org).await
}
async fn user_teams<T: AuthTransaction>(tx: &mut T, user: &str) -> BlueprintResult<Vec<Value>> {
    let mut result = Vec::new();
    for row in tx
        .list(Model::TeamMember)
        .await?
        .into_iter()
        .filter(|row| row["userId"] == user)
        .take(100)
    {
        let team = tx
            .get(Model::Team, text(&row, "teamId")?)
            .await?
            .ok_or_else(|| {
                BlueprintError::new("HTTP_INTERNAL_EMPTY", "Team membership has no team")
            })?;
        result.push(public_team(team));
    }
    Ok(result)
}
