//! Idempotent bootstrap of roles and login users from a TOML seed file, run by
//! a generated service as `iam --seed`. Rerunning updates roles, profiles and
//! passwords in place; nothing is deleted. Passwords are never stored in the
//! file: each user names the process variable that holds its password.
//!
//! ```toml
//! [[roles]]
//! name = "admin"
//! permissions = ["platform:read", "platform:write"]
//!
//! [[users]]
//! id = "seed-admin"
//! email = "admin@example.com"
//! display_name = "Administrator"
//! roles = ["admin"]
//! password_env = "IAM_SEED_ADMIN_PASSWORD"
//! ```
use forklaunch_core::domain::{BlueprintError, BlueprintResult};
use forklaunch_iam_interface::{AsyncIamRepository, Permission, Role, User};
use serde::Deserialize;

/// The roles and users a seed run creates or updates.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedConfig {
    #[serde(default)]
    pub roles: Vec<SeedRole>,
    #[serde(default)]
    pub users: Vec<SeedUser>,
}

/// A role and its `resource:action` permission slugs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedRole {
    pub name: String,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// A login user. `id` is used only when the email is new; an existing user
/// with the same email keeps its id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedUser {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    /// Process variable holding the password; required and non-empty at seed time.
    pub password_env: String,
}

/// What a seed run did, for the operator's log. Never contains passwords.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedReport {
    pub roles: Vec<String>,
    pub created_users: Vec<String>,
    pub updated_users: Vec<String>,
}

fn invalid(message: impl Into<String>) -> BlueprintError {
    BlueprintError::new("iam_seed_invalid", message)
}

impl SeedConfig {
    /// Parses and validates a seed file.
    ///
    /// # Errors
    /// Rejects malformed TOML, unknown fields, blank names, permission slugs
    /// without `resource:action` and duplicate emails. User roles may name roles
    /// that already exist in storage, so they are not cross-checked.
    pub fn from_toml(source: &str) -> BlueprintResult<Self> {
        let config: Self = toml::from_str(source)
            .map_err(|error| invalid(format!("invalid seed file: {error}")))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> BlueprintResult<()> {
        let blank = |value: &str| value.trim().is_empty();
        for role in &self.roles {
            if blank(&role.name) {
                return Err(invalid("seed role names cannot be blank"));
            }
            for permission in &role.permissions {
                if split_permission(permission).is_none() {
                    return Err(invalid(format!(
                        "seed permission {permission:?} must be resource:action"
                    )));
                }
            }
        }
        let mut emails = std::collections::HashSet::new();
        for user in &self.users {
            if blank(&user.id) || blank(&user.email) || blank(&user.password_env) {
                return Err(invalid("seed users need id, email and password_env"));
            }
            if !emails.insert(user.email.to_ascii_lowercase()) {
                return Err(invalid(format!(
                    "seed email {} is listed twice",
                    user.email
                )));
            }
        }
        Ok(())
    }

    /// Creates or updates every role, then every user, reading each password
    /// through `password`. Idempotent.
    ///
    /// # Errors
    /// Fails before writing anything when a password variable is unset or
    /// empty; storage errors stop the run (earlier writes remain).
    pub async fn apply<R, F>(&self, iam: &R, password: F) -> BlueprintResult<SeedReport>
    where
        R: AsyncIamRepository,
        F: Fn(&str) -> Option<String>,
    {
        let mut passwords = Vec::with_capacity(self.users.len());
        for user in &self.users {
            let value = password(&user.password_env)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid(format!("{} is not set or is empty", user.password_env)))?;
            passwords.push(value);
        }
        let mut report = SeedReport::default();
        for seed in &self.roles {
            let mut role = Role::new(seed.name.trim());
            role.permissions = seed
                .permissions
                .iter()
                .filter_map(|slug| split_permission(slug))
                .map(|(resource, action)| Permission::new(resource, action))
                .collect();
            iam.put_role(role).await?;
            report.roles.push(seed.name.trim().to_owned());
        }
        for (seed, password) in self.users.iter().zip(passwords) {
            let mut user = User::new(seed.id.trim(), seed.email.trim());
            user.display_name.clone_from(&seed.display_name);
            user.roles.clone_from(&seed.roles);
            if let Some(existing) = iam.find_user_by_email(seed.email.trim()).await? {
                user.id.clone_from(&existing.id);
                iam.update_user(user).await?;
                iam.set_password(&existing.id, password).await?;
                report.updated_users.push(seed.email.clone());
            } else {
                iam.create_user(user, password).await?;
                report.created_users.push(seed.email.clone());
            }
        }
        Ok(report)
    }
}

fn split_permission(slug: &str) -> Option<(&str, &str)> {
    let (resource, action) = slug.split_once(':')?;
    (!resource.trim().is_empty() && !action.trim().is_empty())
        .then(|| (resource.trim(), action.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_files_are_validated_before_any_write() {
        let config = SeedConfig::from_toml(
            "[[roles]]\nname='admin'\npermissions=['platform:read','platform:write']\n\
             [[users]]\nid='a'\nemail='a@example.test'\nroles=['admin']\npassword_env='A_PASSWORD'\n",
        )
        .unwrap();
        assert_eq!(config.roles[0].permissions.len(), 2);
        for invalid in [
            "[[roles]]\nname=' '\n",
            "[[roles]]\nname='x'\npermissions=['platform']\n",
            "[[users]]\nid='a'\nemail='a@x'\npassword_env=''\n",
            "[[users]]\nid='a'\nemail='a@x'\npassword_env='P'\n[[users]]\nid='b'\nemail='A@x'\npassword_env='P'\n",
            "unknown = 1\n",
        ] {
            assert!(SeedConfig::from_toml(invalid).is_err(), "{invalid}");
        }
    }
}
