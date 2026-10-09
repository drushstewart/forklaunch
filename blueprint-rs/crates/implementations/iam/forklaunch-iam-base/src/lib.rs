#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub mod credentials;
#[cfg(feature = "postgres")]
pub mod postgres;

use forklaunch_blueprint_core::{BlueprintResult, Page, PageRequest};
use forklaunch_iam_interface::{
    Credentials, IamService, Organization, Permission, Role, Session, User, invalid_credentials,
};

#[derive(Debug, Default)]
pub struct InMemoryIamService {
    users: Vec<User>,
    passwords_by_email: BTreeMap<String, String>,
    permissions_by_user_id: BTreeMap<String, Vec<Permission>>,
    organizations: Vec<Organization>,
    roles_by_user_id: BTreeMap<String, Vec<Role>>,
}

impl InMemoryIamService {
    #[must_use]
    pub fn new(users: Vec<User>) -> Self {
        Self {
            users,
            passwords_by_email: BTreeMap::new(),
            permissions_by_user_id: BTreeMap::new(),
            organizations: Vec::new(),
            roles_by_user_id: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_organization(mut self, organization: Organization) -> Self {
        self.organizations.push(organization);
        self
    }

    #[must_use]
    pub fn with_role(mut self, user_id: impl Into<String>, role: Role) -> Self {
        self.roles_by_user_id
            .entry(user_id.into())
            .or_default()
            .push(role);
        self
    }

    /// Stores a salted Argon2id password hash rather than the raw credential.
    ///
    /// # Errors
    /// Returns an error if secure randomness or password hashing fails.
    pub fn with_password(
        mut self,
        email: impl Into<String>,
        password: impl AsRef<str>,
    ) -> BlueprintResult<Self> {
        self.passwords_by_email
            .insert(email.into(), credentials::hash_password(password.as_ref())?);
        Ok(self)
    }

    #[must_use]
    pub fn with_permission(mut self, user_id: impl Into<String>, permission: Permission) -> Self {
        self.permissions_by_user_id
            .entry(user_id.into())
            .or_default()
            .push(permission);
        self
    }
}

impl IamService for InMemoryIamService {
    fn get_user(&self, id: &str) -> BlueprintResult<Option<User>> {
        Ok(self.users.iter().find(|user| user.id == id).cloned())
    }

    fn find_user_by_email(&self, email: &str) -> BlueprintResult<Option<User>> {
        Ok(self.users.iter().find(|user| user.email == email).cloned())
    }

    fn list_users(&self, request: PageRequest) -> BlueprintResult<Page<User>> {
        let start = request
            .cursor
            .as_ref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .unwrap_or(0)
            .min(self.users.len());
        let end = start.saturating_add(request.limit).min(self.users.len());
        let mut page = Page::new(self.users[start..end].to_vec());

        if end < self.users.len() {
            page = page.with_next_cursor(end.to_string());
        }

        Ok(page)
    }

    fn authenticate(&self, credentials: Credentials) -> BlueprintResult<Session> {
        let expected_password = self
            .passwords_by_email
            .get(&credentials.email)
            .ok_or_else(invalid_credentials)?;
        if !credentials::verify_password(&credentials.password, expected_password) {
            return Err(invalid_credentials());
        }

        let user = self
            .find_user_by_email(&credentials.email)?
            .ok_or_else(invalid_credentials)?;

        if user.disabled {
            return Err(invalid_credentials());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid_credentials())?
            .as_secs();
        Ok(Session {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: user.id,
            expires_at_unix_seconds: now.saturating_add(3600),
        })
    }

    fn permissions_for_user(&self, user_id: &str) -> BlueprintResult<Vec<Permission>> {
        Ok(self
            .permissions_by_user_id
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }

    fn get_organization(&self, id: &str) -> BlueprintResult<Option<Organization>> {
        Ok(self
            .organizations
            .iter()
            .find(|organization| organization.id == id)
            .cloned())
    }

    fn list_organizations(&self, request: PageRequest) -> BlueprintResult<Page<Organization>> {
        let start = request
            .cursor
            .as_ref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .unwrap_or(0)
            .min(self.organizations.len());
        let end = start
            .saturating_add(request.limit)
            .min(self.organizations.len());
        let mut page = Page::new(self.organizations[start..end].to_vec());

        if end < self.organizations.len() {
            page = page.with_next_cursor(end.to_string());
        }

        Ok(page)
    }

    fn roles_for_user(&self, user_id: &str) -> BlueprintResult<Vec<Role>> {
        Ok(self
            .roles_by_user_id
            .get(user_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_iam_authenticates_and_lists_users() {
        let service = InMemoryIamService::new(vec![User::new("user_1", "test@example.com")])
            .with_password("test@example.com", "secret")
            .expect("password hash")
            .with_permission("user_1", Permission::new("accounts", "read"));

        let session = service
            .authenticate(Credentials {
                email: "test@example.com".to_owned(),
                password: "secret".to_owned(),
            })
            .expect("credentials should be valid");
        let page = service
            .list_users(PageRequest::first_page(10))
            .expect("list should succeed");

        assert_eq!(session.user_id, "user_1");
        assert_eq!(page.items.len(), 1);
        assert_eq!(
            service
                .permissions_for_user("user_1")
                .expect("permissions should load")
                .len(),
            1
        );
    }

    #[test]
    fn in_memory_iam_resolves_organizations_and_roles() {
        use forklaunch_iam_interface::{Organization, Role};

        let service = InMemoryIamService::new(vec![User::new("user_1", "test@example.com")])
            .with_organization(Organization::new("org_1", "Acme", "acme"))
            .with_role(
                "user_1",
                Role::new("admin").with_permission(Permission::new("accounts", "read")),
            );

        assert_eq!(
            service
                .get_organization("org_1")
                .expect("org lookup should succeed")
                .map(|organization| organization.slug),
            Some("acme".to_owned())
        );
        assert_eq!(
            service
                .list_organizations(PageRequest::first_page(10))
                .expect("org list should succeed")
                .items
                .len(),
            1
        );
        assert_eq!(
            service
                .roles_for_user("user_1")
                .expect("roles should load")
                .len(),
            1
        );
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    #[test]
    fn authentication_issues_unique_short_lived_sessions_and_rejects_disabled_users() {
        let credentials = || Credentials {
            email: "test@example.com".to_owned(),
            password: "secret".to_owned(),
        };
        let service = InMemoryIamService::new(vec![User::new("user_1", "test@example.com")])
            .with_password("test@example.com", "secret")
            .unwrap();
        let first = service.authenticate(credentials()).unwrap();
        let second = service.authenticate(credentials()).unwrap();
        assert_ne!(first.id, second.id);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(first.expires_at_unix_seconds > now);
        assert!(first.expires_at_unix_seconds <= now + 3600);
        let mut disabled = User::new("user_1", "test@example.com");
        disabled.disabled = true;
        let service = InMemoryIamService::new(vec![disabled])
            .with_password("test@example.com", "secret")
            .unwrap();
        assert!(service.authenticate(credentials()).is_err());
    }

    #[test]
    fn out_of_range_pagination_does_not_panic() {
        let service = InMemoryIamService::default();
        let request = PageRequest {
            limit: 10,
            cursor: Some(usize::MAX.to_string()),
        };
        assert!(
            service
                .list_users(request.clone())
                .unwrap()
                .items
                .is_empty()
        );
        assert!(
            service
                .list_organizations(request)
                .unwrap()
                .items
                .is_empty()
        );
    }
}
