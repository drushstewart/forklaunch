//! Dynamic role updates and assignment-protected deletion.
use super::create::{check_permissions, check_predefined, denied, taken};
use super::{
    AuthStore, AuthTransaction, BlueprintError, BlueprintResult, DateTime, Model,
    OrganizationService, Utc, Value, allowed, json, not_found, public, scope, text, unauthorized,
};
impl<S: AuthStore> OrganizationService<S> {
    /// Updates all matching stored roles while leaving existing member role names intact.
    /// # Errors
    /// Requires raw membership/ac:update, valid stored JSON and requested grants/names.
    pub async fn update_organization_role(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.organization_features.require_roles()?;
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = scope(body["organizationId"].as_str(), &view.session)?;
        let member_role = authorize(
            &self.access_control,
            &mut tx,
            &org,
            text(&view.user, "id")?,
            "update",
        )
        .await?;
        let (field, key) = selector(body)?;
        let rows: Vec<_> =
            crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
                .await?
                .into_iter()
                .filter(|row| row["organizationId"] == org && row[field] == key)
                .collect();
        let old = rows.first().ok_or_else(not_found)?;
        let old_permission = match old["permission"].as_str() {
            Some("") | None if old["permission"].is_null() || old["permission"] == "" => {
                Value::Null
            }
            Some(encoded) => crate::ecmascript::parse(encoded).map_err(|_| {
                BlueprintError::new("HTTP_INTERNAL_EMPTY", "Invalid role permissions")
            })?,
            _ => {
                return Err(BlueprintError::new(
                    "HTTP_INTERNAL_EMPTY",
                    "Invalid role permissions",
                ));
            }
        };
        let data = &body["data"];
        if let Some(permission) = data.get("permission") {
            check_permissions(
                &self.access_control,
                &mut tx,
                &org,
                &member_role,
                permission,
                "update",
            )
            .await?;
        }
        let name = data["roleName"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_lowercase);
        if let Some(name) = &name {
            check_predefined(&self.access_control, name)?;
            if crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
                .await?
                .iter()
                .any(|row| row["organizationId"] == org && row["role"] == *name)
            {
                return Err(taken());
            }
        }
        let mut result = old.clone();
        if let Some(name) = &name {
            result["role"] = json!(name);
        }
        result["permission"] = data.get("permission").map_or_else(
            || {
                if matches!(old_permission, Value::Null | Value::Bool(false))
                    || old_permission == ""
                    || old_permission
                        .as_number()
                        .is_some_and(|number| number.as_f64().is_some_and(|value| value == 0.0))
                {
                    Value::Null
                } else {
                    old_permission
                }
            },
            super::super::permission::admitted_record,
        );
        for mut row in rows {
            if let Some(name) = &name {
                row["role"] = json!(name);
            }
            if let Some(permission) = data.get("permission") {
                row["permission"] = json!(crate::ecmascript::stringify(
                    &super::super::permission::admitted_record(permission)
                ));
            }
            row["updatedAt"] = json!(super::super::timestamp(now));
            tx.replace(Model::OrganizationRole, row).await?;
        }
        tx.commit().await?;
        Ok(json!({"success":true,"roleData":result}))
    }
    /// Deletes matching stored roles after the configured first-100 assignment guard.
    /// # Errors
    /// Requires raw membership/ac:delete, a mutable selector and valid stored JSON.
    pub async fn delete_organization_role(
        &self,
        token: &str,
        transient: bool,
        body: &Value,
        now: DateTime<Utc>,
    ) -> BlueprintResult<Value> {
        self.organization_features.require_roles()?;
        self.session(token, transient, now).await?;
        let mut tx = self.store.begin(true).await?;
        let view = self
            .recheck(&mut tx, token, now)
            .await?
            .ok_or_else(unauthorized)?;
        let org = scope(body["organizationId"].as_str(), &view.session)?;
        authorize(
            &self.access_control,
            &mut tx,
            &org,
            text(&view.user, "id")?,
            "delete",
        )
        .await?;
        let (field, key) = selector(body)?;
        if field == "role" && check_predefined(&self.access_control, key).is_err() {
            return Err(BlueprintError::new(
                "CANNOT_DELETE_A_PRE_DEFINED_ROLE",
                "Cannot delete a pre-defined role",
            ));
        }
        let rows: Vec<_> =
            crate::store::find_all(&mut tx, Model::OrganizationRole, "organizationId", &org)
                .await?
                .into_iter()
                .filter(|row| row["organizationId"] == org && row[field] == key)
                .collect();
        let row = public(rows.first().ok_or_else(not_found)?.clone())?;
        let name = text(&row, "role")?;
        if crate::store::find_all(&mut tx, Model::Member, "organizationId", &org)
            .await?
            .iter()
            .filter(|row| {
                row["organizationId"] == org
                    && row["role"].as_str().is_some_and(|role| role.contains(name))
            })
            .take(100)
            .any(|row| {
                row["role"]
                    .as_str()
                    .is_some_and(|roles| roles.split(',').any(|role| trim(role) == name))
            })
        {
            return Err(BlueprintError::new(
                "ROLE_IS_ASSIGNED_TO_MEMBERS",
                "Cannot delete a role that is assigned to members. Please reassign the members to a different role first",
            ));
        }
        for row in rows {
            tx.delete(Model::OrganizationRole, text(&row, "id")?)
                .await?;
        }
        tx.commit().await?;
        Ok(json!({"success":true}))
    }
}
fn selector(body: &Value) -> BlueprintResult<(&str, &str)> {
    body["roleName"]
        .as_str()
        .filter(|name| !name.is_empty())
        .map(|name| ("role", name))
        .or_else(|| {
            body["roleId"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(|id| ("id", id))
        })
        .ok_or_else(not_found)
}
fn trim(value: &str) -> &str {
    value.trim_matches(|c: char| c == '\u{feff}' || c.is_whitespace() && c != '\u{85}')
}
async fn authorize<T: AuthTransaction>(
    config: &crate::organizations::Authorization,
    tx: &mut T,
    org: &str,
    user: &str,
    action: &str,
) -> BlueprintResult<String> {
    let member = crate::store::find_all(tx, Model::Member, "organizationId", org)
        .await?
        .into_iter()
        .find(|row| row["organizationId"] == org && row["userId"] == user)
        .ok_or_else(|| {
            BlueprintError::new(
                "YOU_ARE_NOT_A_MEMBER_OF_THIS_ORGANIZATION",
                "You are not a member of this organization",
            )
        })?;
    let role = text(&member, "role")?;
    if !allowed(config, tx, org, role, Some(&json!({"ac":[action]}))).await? {
        return Err(denied(action));
    }
    Ok(role.to_owned())
}
