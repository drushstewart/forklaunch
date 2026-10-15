use super::{AuthTransaction, BlueprintResult, Model, Value, error};
pub(super) fn roles(value: &Value) -> String {
    if let Some(roles) = value.as_array() {
        roles
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(",")
    } else {
        value.as_str().unwrap_or("").to_owned()
    }
}
pub(super) fn trim(value: &str) -> &str {
    value.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}')
}
pub(super) async fn validate_roles<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    org: &str,
    role: &str,
) -> BlueprintResult<()> {
    let unknown = role
        .split(',')
        .map(trim)
        .filter(|role| !role.is_empty() && !config.admitted_role(role))
        .collect::<Vec<_>>();
    let found = if config.dynamic_enabled {
        crate::store::find_all(tx, Model::OrganizationRole, "organizationId", org)
            .await?
            .into_iter()
            .filter(|row| {
                row["organizationId"] == org
                    && row["role"]
                        .as_str()
                        .is_some_and(|role| unknown.contains(&role))
            })
            .take(100)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let invalid = unknown
        .into_iter()
        .filter(|role| !found.iter().any(|row| row["role"] == *role))
        .collect::<Vec<_>>();
    if !invalid.is_empty() {
        return Err(error(
            "HTTP_MESSAGE_ONLY",
            &format!("ROLE_NOT_FOUND: {}", invalid.join(", ")),
        ));
    }
    Ok(())
}
pub(super) async fn teams<T: AuthTransaction>(
    tx: &mut T,
    org: &str,
    value: Option<&Value>,
    limit: Option<u32>,
) -> BlueprintResult<Vec<String>> {
    let teams = match value {
        Some(Value::String(value)) => vec![value.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    };
    if value.is_some_and(|value| value.is_array() || value.as_str().is_some_and(|s| !s.is_empty()))
    {
        if teams.iter().any(|id| id.contains(',')) {
            return Err(error(
                "INVALID_TEAM_ID",
                "Team id contains a reserved character",
            ));
        }
        for id in &teams {
            if !tx
                .get(Model::Team, id)
                .await?
                .is_some_and(|row| row["organizationId"] == org)
            {
                return Err(error("TEAM_NOT_FOUND", "Team not found"));
            }
        }
    }
    if let Some(limit) = limit
        && value
            .is_some_and(|value| value.is_array() || value.as_str().is_some_and(|s| !s.is_empty()))
    {
        for id in &teams {
            let count = crate::store::find_all(tx, Model::TeamMember, "teamId", id)
                .await?
                .iter()
                .filter(|row| row["teamId"] == *id)
                .take(100)
                .count();
            if u64::try_from(count).unwrap_or(u64::MAX) >= u64::from(limit) {
                return Err(super::super::super::team_policy::member_limit());
            }
        }
    }
    Ok(teams)
}

pub(super) async fn ensure_not_member<T: AuthTransaction>(
    tx: &mut T,
    org: &str,
    email: &str,
) -> BlueprintResult<()> {
    if let Some(user) = tx.find_unique(Model::User, "email", email).await?
        && crate::store::find_all(tx, Model::Member, "organizationId", org)
            .await?
            .iter()
            .any(|row| row["organizationId"] == org && row["userId"] == user["id"])
    {
        return Err(error(
            "USER_IS_ALREADY_A_MEMBER_OF_THIS_ORGANIZATION",
            "User is already a member of this organization",
        ));
    }
    Ok(())
}
