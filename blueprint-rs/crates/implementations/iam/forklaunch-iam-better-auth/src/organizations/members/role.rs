use super::super::permission::allowed_with_creator;
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, find_member, member_missing, missing, no_active, owner_count,
    owns, text, trim, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Changes member roles while preserving creator/last-owner protections.
    /// Configured owners can update members after stored grants are validated.
    /// # Errors
    /// Returns session, member, role, creator or permission errors.
    pub async fn update_member_role(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.session(token, transient, now).await?;
        if body["role"].as_str() == Some("") {
            return Err(empty());
        }
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let id = body["organizationId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .or_else(|| {
                view.session["activeOrganizationId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            })
            .ok_or_else(no_active)?;
        let roles = parse_roles(&body["role"])?;
        let actor = find_member(&mut tx, text(&view.user, "id")?, id)
            .await?
            .ok_or_else(member_missing)?;
        let member_id = text(body, "memberId")?;
        let mut target = tx
            .get(Model::Member, member_id)
            .await?
            .ok_or_else(member_missing)?;
        if target["id"] != actor["id"]
            && tx
                .get(Model::User, text(&target, "userId")?)
                .await?
                .is_none()
        {
            return Err(BlueprintError::new(
                "HTTP_INTERNAL_EMPTY",
                "Member user not found",
            ));
        }
        if target["organizationId"] != id {
            return Err(forbidden());
        }
        let creator = owns(&actor, self.creator_role.as_str());
        let setting_creator = roles.iter().any(|role| role == self.creator_role.as_str());
        if (owns(&target, self.creator_role.as_str()) || setting_creator) && !creator {
            return Err(forbidden());
        }
        if creator
            && actor["id"] == target["id"]
            && !setting_creator
            && owner_count(&mut tx, id, self.creator_role.as_str()).await? <= 1
        {
            return Err(BlueprintError::new(
                "YOU_CANNOT_LEAVE_THE_ORGANIZATION_WITHOUT_AN_OWNER",
                "You cannot leave the organization without an owner",
            ));
        }
        if !allowed_with_creator(
            &self.access_control,
            &mut tx,
            id,
            text(&actor, "role")?,
            Some(&serde_json::json!({"member":["update"]})),
            Some(self.creator_role.as_str()),
        )
        .await?
        {
            return Err(forbidden());
        }
        validate_roles(&self.access_control, &mut tx, id, &roles).await?;
        if tx.get(Model::Organization, id).await?.is_none() {
            return Err(missing());
        }
        if tx
            .get(Model::User, text(&target, "userId")?)
            .await?
            .is_none()
        {
            return Err(BlueprintError::new("HTTP_MESSAGE_ONLY", "User not found"));
        }
        target["role"] = serde_json::json!(roles.join(","));
        tx.replace(Model::Member, target.clone()).await?;
        tx.commit().await?;
        Ok(target)
    }
}
fn parse_roles(value: &Value) -> BlueprintResult<Vec<String>> {
    let values: Vec<&str> = match value {
        Value::String(value) => vec![value],
        Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
        _ => return Err(empty()),
    };
    let roles: Vec<_> = values
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(trim)
        .filter(|role| !role.is_empty())
        .map(str::to_owned)
        .collect();
    if roles.is_empty() {
        return Err(empty());
    }
    Ok(roles)
}
async fn validate_roles<T: AuthTransaction>(
    config: &crate::organizations::OrganizationAccessControl,
    tx: &mut T,
    organization: &str,
    roles: &[String],
) -> BlueprintResult<()> {
    let unknown: Vec<_> = roles
        .iter()
        .filter(|role| !config.admitted_role(role))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let found: Vec<_> =
        crate::store::find_all(tx, Model::OrganizationRole, "organizationId", organization)
            .await?
            .into_iter()
            .filter(|row| {
                row["organizationId"] == organization
                    && unknown
                        .iter()
                        .any(|role| row["role"].as_str() == Some(role))
            })
            .take(100)
            .filter_map(|row| row["role"].as_str().map(str::to_owned))
            .collect();
    let invalid: Vec<_> = unknown
        .into_iter()
        .filter(|role| !found.contains(role))
        .map(String::as_str)
        .collect();
    if !invalid.is_empty() {
        return Err(BlueprintError::new(
            "ROLE_NOT_FOUND",
            format!("ROLE_NOT_FOUND: {}", invalid.join(", ")),
        ));
    }
    Ok(())
}
fn forbidden() -> BlueprintError {
    BlueprintError::new(
        "YOU_ARE_NOT_ALLOWED_TO_UPDATE_THIS_MEMBER",
        "You are not allowed to update this member",
    )
}
fn empty() -> BlueprintError {
    BlueprintError::new("HTTP_BAD_REQUEST_EMPTY", "Bad Request")
}
