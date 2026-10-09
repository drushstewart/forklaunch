#![forbid(unsafe_code)]

use forklaunch_blueprint_core::{BlueprintError, BlueprintResult, Page, PageRequest};

/// An organization (tenant boundary) a user can belong to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub slug: String,
}

impl Organization {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, slug: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            slug: slug.into(),
        }
    }
}

/// A named role bundling a set of permissions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Role {
    pub name: String,
    pub permissions: Vec<Permission>,
}

impl Role {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            permissions: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_permission(mut self, permission: Permission) -> Self {
        self.permissions.push(permission);
        self
    }
}

/// An issued bearer token bound to a user.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub value: String,
    pub user_id: String,
    pub expires_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub disabled: bool,
    pub roles: Vec<String>,
}

impl User {
    #[must_use]
    pub fn new(id: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            email: email.into(),
            display_name: None,
            disabled: false,
            roles: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_display_name(mut self, display_name: impl Into<String>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.push(role.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub expires_at_unix_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Permission {
    pub resource: String,
    pub action: String,
}

impl Permission {
    #[must_use]
    pub fn new(resource: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            resource: resource.into(),
            action: action.into(),
        }
    }
}

pub trait IamService {
    /// # Errors
    ///
    /// Returns an error when the user backend cannot complete the lookup.
    fn get_user(&self, id: &str) -> BlueprintResult<Option<User>>;

    /// # Errors
    ///
    /// Returns an error when the user backend cannot complete the lookup.
    fn find_user_by_email(&self, email: &str) -> BlueprintResult<Option<User>>;

    /// # Errors
    ///
    /// Returns an error when the user backend cannot list users.
    fn list_users(&self, request: PageRequest) -> BlueprintResult<Page<User>>;

    /// # Errors
    ///
    /// Returns an error when credentials are invalid or the backend cannot create a session.
    fn authenticate(&self, credentials: Credentials) -> BlueprintResult<Session>;

    /// # Errors
    ///
    /// Returns an error when permission lookup fails.
    fn permissions_for_user(&self, user_id: &str) -> BlueprintResult<Vec<Permission>>;

    /// # Errors
    ///
    /// Returns an error when the organization backend cannot complete the lookup.
    fn get_organization(&self, _id: &str) -> BlueprintResult<Option<Organization>> {
        Ok(None)
    }

    /// # Errors
    ///
    /// Returns an error when the organization backend cannot list organizations.
    fn list_organizations(&self, _request: PageRequest) -> BlueprintResult<Page<Organization>> {
        Ok(Page::new(Vec::new()))
    }

    /// # Errors
    ///
    /// Returns an error when role resolution fails.
    fn roles_for_user(&self, _user_id: &str) -> BlueprintResult<Vec<Role>> {
        Ok(Vec::new())
    }
}

#[must_use]
pub fn invalid_credentials() -> BlueprintError {
    BlueprintError::new("iam_invalid_credentials", "invalid credentials")
}

/// IAM HTTP route contracts, the source of truth for `OpenAPI` and SDK generation.
pub mod contracts {
    use forklaunch_blueprint_core::contract::{
        AccessLevel, AuthConfig, BodyContract, ContractRegistry, FieldContract, JwtAuthConfig,
        RouteContract, ServiceContract,
    };

    /// Builds the IAM service contract registry.
    #[must_use]
    pub fn registry() -> ContractRegistry {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("iam", "0.1.0"));

        registry.register(
            RouteContract::post(
                "Authenticate",
                "Exchange credentials for a session",
                "/auth/login",
            )
            .with_request_body(BodyContract::json("Credentials"))
            .with_response(200, Some(BodyContract::json("Session")))
            .with_response(401, None),
        );

        registry.register(
            RouteContract::get("Get User", "Get a user by id", "/users/:id")
                .with_access(AccessLevel::Protected)
                .with_auth(AuthConfig::jwt(JwtAuthConfig::SignatureKey))
                .with_param(FieldContract::required("id", "String"))
                .with_response(200, Some(BodyContract::json("User")))
                .with_response(404, None),
        );

        registry.register(
            RouteContract::get("List Users", "List users", "/users")
                .with_access(AccessLevel::Protected)
                .with_auth(AuthConfig::jwt(JwtAuthConfig::SignatureKey))
                .with_query(FieldContract::optional("limit", "Integer"))
                .with_query(FieldContract::optional("cursor", "String"))
                .with_response(200, Some(BodyContract::json("UserPage"))),
        );

        registry.register(
            RouteContract::get(
                "List User Permissions",
                "List the permissions granted to a user",
                "/users/:id/permissions",
            )
            .with_access(AccessLevel::Protected)
            .with_auth(AuthConfig::jwt(JwtAuthConfig::SignatureKey))
            .with_param(FieldContract::required("id", "String"))
            .with_response(200, Some(BodyContract::json("PermissionList"))),
        );

        registry.register(
            RouteContract::get("List Organizations", "List organizations", "/organizations")
                .with_access(AccessLevel::Protected)
                .with_auth(AuthConfig::jwt(JwtAuthConfig::SignatureKey))
                .with_response(200, Some(BodyContract::json("OrganizationPage"))),
        );

        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_builder_records_roles() {
        let user = User::new("user_1", "test@example.com")
            .with_display_name("Test User")
            .with_role("admin");

        assert_eq!(user.display_name.as_deref(), Some("Test User"));
        assert_eq!(user.roles, vec!["admin"]);
    }

    #[test]
    fn role_bundles_permissions() {
        let role = Role::new("editor").with_permission(Permission::new("articles", "write"));
        assert_eq!(role.permissions.len(), 1);
    }

    #[test]
    fn iam_contracts_cover_core_routes() {
        let registry = contracts::registry();
        let operation_names: Vec<&str> = registry
            .routes()
            .iter()
            .map(|route| route.operation_name.as_str())
            .collect();

        assert!(operation_names.contains(&"authenticate"));
        assert!(operation_names.contains(&"getUser"));
        assert!(operation_names.contains(&"listUsers"));
        assert!(operation_names.contains(&"listOrganizations"));

        // protected routes carry an auth config
        let get_user = registry
            .routes()
            .iter()
            .find(|route| route.operation_name == "getUser")
            .expect("getUser route should exist");
        assert!(get_user.auth.is_some());
    }
}
