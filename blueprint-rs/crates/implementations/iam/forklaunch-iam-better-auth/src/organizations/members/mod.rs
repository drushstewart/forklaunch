//! Configured membership identity, owner guards and team cleanup.
use super::mutation::find_member;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_slug, json, load, missing, text, unauthorized,
    update_active,
};
mod add;
mod leave;
mod list;
mod read;
mod remove;
mod role;
pub use add::{AddMemberInput, MemberRoles};

fn no_active() -> BlueprintError {
    BlueprintError::new("NO_ACTIVE_ORGANIZATION", "No active organization")
}
fn member_missing() -> BlueprintError {
    BlueprintError::new("MEMBER_NOT_FOUND", "Member not found")
}
fn nonmember() -> BlueprintError {
    BlueprintError::new(
        "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION",
        "You are not a member of this organization",
    )
}
fn only_owner() -> BlueprintError {
    BlueprintError::new(
        "YOU_CANNOT_LEAVE_THE_ORGANIZATION_AS_THE_ONLY_OWNER",
        "You cannot leave the organization as the only owner",
    )
}
fn owns(member: &Value) -> bool {
    member["role"]
        .as_str()
        .is_some_and(|roles| roles.split(',').any(|role| role == "owner"))
}
fn trim(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character == '\u{feff}' || character.is_whitespace() && character != '\u{85}'
    })
}
async fn joined<T: AuthTransaction>(
    tx: &mut T,
    mut member: Value,
) -> BlueprintResult<Option<Value>> {
    let Some(user) = tx.get(Model::User, text(&member, "userId")?).await? else {
        return Ok(None);
    };
    let mut public = json!({});
    for field in ["id", "name", "email", "image"] {
        if let Some(value) = user.get(field) {
            public[field] = value.clone();
        }
    }
    member["user"] = public;
    Ok(Some(member))
}
async fn owner_count<T: AuthTransaction>(tx: &mut T, organization: &str) -> BlueprintResult<usize> {
    Ok(tx
        .list(Model::Member)
        .await?
        .iter()
        .filter(|row| row["organizationId"] == organization)
        .take(100)
        .filter(|row| owns(row))
        .count())
}
async fn delete_member<T: AuthTransaction>(tx: &mut T, member: &Value) -> BlueprintResult<()> {
    tx.delete(Model::Member, text(member, "id")?).await?;
    let user = text(member, "userId")?;
    for mut team in tx
        .list(Model::Team)
        .await?
        .into_iter()
        .filter(|row| row["organizationId"] == member["organizationId"])
        .take(100)
    {
        let id = text(&team, "id")?.to_owned();
        let mut removed = 0_i64;
        for row in tx.list(Model::TeamMember).await? {
            if row["teamId"] == id && row["userId"] == user {
                tx.delete(Model::TeamMember, text(&row, "id")?).await?;
                removed += 1;
            }
        }
        if removed > 0
            && let Some(count) = team["memberCount"]
                .as_i64()
                .filter(|count| *count >= removed)
        {
            team["memberCount"] = json!(count - removed);
            tx.replace(Model::Team, team).await?;
        }
    }
    Ok(())
}
