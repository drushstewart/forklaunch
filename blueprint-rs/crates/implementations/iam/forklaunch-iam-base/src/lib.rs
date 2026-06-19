#![forbid(unsafe_code)]

use std::collections::BTreeMap;

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

    #[must_use]
    pub fn with_password(mut self, email: impl Into<String>, password: impl Into<String>) -> Self {
        self.passwords_by_email
            .insert(email.into(), password.into());
        self
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
            .unwrap_or(0);
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
        if expected_password != &credentials.password {
            return Err(invalid_credentials());
        }

        let user = self
            .find_user_by_email(&credentials.email)?
            .ok_or_else(invalid_credentials)?;

        Ok(Session {
            id: format!("session_{}", user.id),
            user_id: user.id,
            expires_at_unix_seconds: 4_102_444_800,
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
            .unwrap_or(0);
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
