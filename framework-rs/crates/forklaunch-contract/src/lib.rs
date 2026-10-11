#![forbid(unsafe_code)]

#[cfg(feature = "json-schema")]
mod schema;
#[cfg(feature = "json-schema")]
pub use schema::relocate_schema_json;

use std::collections::BTreeMap;

use forklaunch_common::{
    json_array, json_string, json_string_array, open_api_compliant_path, sanitize_path_slashes,
    to_pretty_camel_case,
};

pub const CONTRACT_ARTIFACT_VERSION: &str = "forklaunch.contract.v1";

pub trait ContractSchema {
    #[must_use]
    fn schema_ref() -> SchemaRef;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    Trace,
    Checkout,
    Copy,
    Link,
    Lock,
    MSearch,
    Merge,
    MkActivity,
    MkCol,
    Move,
    Notify,
    PropFind,
    PropPatch,
    Purge,
    Report,
    Search,
    Subscribe,
    Unlink,
    Unlock,
    Unsubscribe,
}

impl HttpMethod {
    /// All HTTP methods exported by the TypeScript router.
    pub const ALL: [Self; 27] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Head,
        Self::Options,
        Self::Trace,
        Self::Checkout,
        Self::Copy,
        Self::Link,
        Self::Lock,
        Self::MSearch,
        Self::Merge,
        Self::MkActivity,
        Self::MkCol,
        Self::Move,
        Self::Notify,
        Self::PropFind,
        Self::PropPatch,
        Self::Purge,
        Self::Report,
        Self::Search,
        Self::Subscribe,
        Self::Unlink,
        Self::Unlock,
        Self::Unsubscribe,
    ];

    /// Parses an exact, case-sensitive HTTP method token.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|method| method.as_str() == value)
    }

    /// Whether `OpenAPI` defines a standard operation key for this method.
    #[must_use]
    pub const fn is_openapi_standard(self) -> bool {
        matches!(
            self,
            Self::Get
                | Self::Post
                | Self::Put
                | Self::Patch
                | Self::Delete
                | Self::Head
                | Self::Options
                | Self::Trace
        )
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
            Self::Checkout => "CHECKOUT",
            Self::Copy => "COPY",
            Self::Link => "LINK",
            Self::Lock => "LOCK",
            Self::MSearch => "M-SEARCH",
            Self::Merge => "MERGE",
            Self::MkActivity => "MKACTIVITY",
            Self::MkCol => "MKCOL",
            Self::Move => "MOVE",
            Self::Notify => "NOTIFY",
            Self::PropFind => "PROPFIND",
            Self::PropPatch => "PROPPATCH",
            Self::Purge => "PURGE",
            Self::Report => "REPORT",
            Self::Search => "SEARCH",
            Self::Subscribe => "SUBSCRIBE",
            Self::Unlink => "UNLINK",
            Self::Unlock => "UNLOCK",
            Self::Unsubscribe => "UNSUBSCRIBE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessLevel {
    Public,
    Authenticated,
    Protected,
    Internal,
}

impl AccessLevel {
    #[must_use]
    pub const fn as_artifact_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Authenticated => "authenticated",
            Self::Protected => "protected",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    Error,
    Warning,
    None,
}

impl ValidationMode {
    #[must_use]
    pub const fn as_artifact_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Json,
    Text,
    File,
    Binary,
    Multipart,
    UrlEncoded,
    ServerSentEvent,
    Unknown,
}

impl BodyKind {
    #[must_use]
    pub const fn as_artifact_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::File => "file",
            Self::Binary => "binary",
            Self::Multipart => "multipart",
            Self::UrlEncoded => "urlEncoded",
            Self::ServerSentEvent => "serverSentEvent",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaRef {
    pub name: String,
    pub json_schema: Option<String>,
}

impl SchemaRef {
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            json_schema: None,
        }
    }

    #[must_use]
    pub fn with_json_schema(mut self, json_schema: impl Into<String>) -> Self {
        self.json_schema = Some(json_schema.into());
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![format!("\"name\":{}", json_string(&self.name))];
        if let Some(json_schema) = &self.json_schema {
            fields.push(format!("\"jsonSchema\":{json_schema}"));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyContract {
    pub kind: BodyKind,
    pub schema: SchemaRef,
    pub content_type: Option<String>,
}

impl BodyContract {
    #[must_use]
    pub fn json(schema_name: impl Into<String>) -> Self {
        Self {
            kind: BodyKind::Json,
            schema: SchemaRef::named(schema_name),
            content_type: Some("application/json".to_owned()),
        }
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!("\"kind\":{}", json_string(self.kind.as_artifact_str())),
            format!("\"schema\":{}", self.schema.to_json()),
        ];
        if let Some(content_type) = &self.content_type {
            fields.push(format!("\"contentType\":{}", json_string(content_type)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldContract {
    pub name: String,
    pub schema: SchemaRef,
    pub required: bool,
}

impl FieldContract {
    #[must_use]
    pub fn required(name: impl Into<String>, schema_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: SchemaRef::named(schema_name),
            required: true,
        }
    }

    #[must_use]
    pub fn optional(name: impl Into<String>, schema_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            schema: SchemaRef::named(schema_name),
            required: false,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"name\":{},\"schema\":{},\"required\":{}}}",
            json_string(&self.name),
            self.schema.to_json(),
            self.required
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestContract {
    pub params: Vec<FieldContract>,
    pub query: Vec<FieldContract>,
    pub headers: Vec<FieldContract>,
    pub body: Option<BodyContract>,
}

impl RequestContract {
    #[must_use]
    pub fn with_body(mut self, body: BodyContract) -> Self {
        self.body = Some(body);
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!(
                "\"params\":{}",
                json_array(self.params.iter().map(FieldContract::to_json))
            ),
            format!(
                "\"query\":{}",
                json_array(self.query.iter().map(FieldContract::to_json))
            ),
            format!(
                "\"headers\":{}",
                json_array(self.headers.iter().map(FieldContract::to_json))
            ),
        ];
        if let Some(body) = &self.body {
            fields.push(format!("\"body\":{}", body.to_json()));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseContract {
    pub status_code: u16,
    pub headers: Vec<FieldContract>,
    pub body: Option<BodyContract>,
}

impl ResponseContract {
    #[must_use]
    pub fn new(status_code: u16, body: Option<BodyContract>) -> Self {
        Self {
            status_code,
            headers: Vec::new(),
            body,
        }
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!("\"statusCode\":{}", self.status_code),
            format!(
                "\"headers\":{}",
                json_array(self.headers.iter().map(FieldContract::to_json))
            ),
        ];
        if let Some(body) = &self.body {
            fields.push(format!("\"body\":{}", body.to_json()));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOptions {
    pub request_validation: ValidationMode,
    pub response_validation: ValidationMode,
    pub openapi: bool,
    pub mcp: bool,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            request_validation: ValidationMode::Error,
            response_validation: ValidationMode::Error,
            openapi: true,
            mcp: true,
        }
    }
}

impl RouteOptions {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"requestValidation\":{},\"responseValidation\":{},\"openapi\":{},\"mcp\":{}}}",
            json_string(self.request_validation.as_artifact_str()),
            json_string(self.response_validation.as_artifact_str()),
            self.openapi,
            self.mcp
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtAuthConfig {
    SignatureKey,
    Jwk,
    JwksUrl { url: String },
}

impl JwtAuthConfig {
    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            Self::SignatureKey => "{\"method\":\"signatureKey\"}".to_owned(),
            Self::Jwk => "{\"method\":\"jwk\"}".to_owned(),
            Self::JwksUrl { url } => {
                format!("{{\"method\":\"jwksUrl\",\"url\":{}}}", json_string(url))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwtAuth {
    pub jwt: JwtAuthConfig,
    pub token_prefix: Option<String>,
    pub header_name: Option<String>,
}

impl JwtAuth {
    #[must_use]
    pub fn new(jwt: JwtAuthConfig) -> Self {
        Self {
            jwt,
            token_prefix: None,
            header_name: None,
        }
    }

    #[must_use]
    pub fn with_token_prefix(mut self, token_prefix: impl Into<String>) -> Self {
        self.token_prefix = Some(token_prefix.into());
        self
    }

    #[must_use]
    pub fn with_header_name(mut self, header_name: impl Into<String>) -> Self {
        self.header_name = Some(header_name.into());
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            "\"method\":\"jwt\"".to_owned(),
            format!("\"jwt\":{}", self.jwt.to_json()),
        ];
        if let Some(token_prefix) = &self.token_prefix {
            fields.push(format!("\"tokenPrefix\":{}", json_string(token_prefix)));
        }
        if let Some(header_name) = &self.header_name {
            fields.push(format!("\"headerName\":{}", json_string(header_name)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BasicAuth;

impl BasicAuth {
    #[must_use]
    pub fn to_json() -> String {
        "{\"method\":\"basic\"}".to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HmacAuth {
    pub header_name: Option<String>,
}

impl HmacAuth {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_header_name(mut self, header_name: impl Into<String>) -> Self {
        self.header_name = Some(header_name.into());
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec!["\"method\":\"hmac\"".to_owned()];
        if let Some(header_name) = &self.header_name {
            fields.push(format!("\"headerName\":{}", json_string(header_name)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthConfig {
    /// Application-verified opaque bearer token, such as a persistent IAM session.
    Bearer,
    Jwt(JwtAuth),
    Basic(BasicAuth),
    Hmac(HmacAuth),
}

impl AuthConfig {
    #[must_use]
    pub const fn bearer() -> Self {
        Self::Bearer
    }

    #[must_use]
    pub fn jwt(jwt: JwtAuthConfig) -> Self {
        Self::Jwt(JwtAuth::new(jwt))
    }

    #[must_use]
    pub const fn basic() -> Self {
        Self::Basic(BasicAuth)
    }

    #[must_use]
    pub fn hmac() -> Self {
        Self::Hmac(HmacAuth::new())
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        match self {
            Self::Bearer => "{\"method\":\"bearer\"}".to_owned(),
            Self::Jwt(jwt) => jwt.to_json(),
            Self::Basic(_) => BasicAuth::to_json(),
            Self::Hmac(hmac) => hmac.to_json(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RbacConfig {
    pub allowed_permissions: Vec<String>,
    pub forbidden_permissions: Vec<String>,
    pub allowed_roles: Vec<String>,
    pub forbidden_roles: Vec<String>,
    pub required_scope: Option<String>,
    pub scope_hierarchy: Vec<String>,
    pub required_features: Vec<String>,
    pub require_active_subscription: bool,
}

impl RbacConfig {
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!(
                "\"allowedPermissions\":{}",
                json_string_array(&self.allowed_permissions)
            ),
            format!(
                "\"forbiddenPermissions\":{}",
                json_string_array(&self.forbidden_permissions)
            ),
            format!(
                "\"allowedRoles\":{}",
                json_string_array(&self.allowed_roles)
            ),
            format!(
                "\"forbiddenRoles\":{}",
                json_string_array(&self.forbidden_roles)
            ),
            format!(
                "\"scopeHierarchy\":{}",
                json_string_array(&self.scope_hierarchy)
            ),
            format!(
                "\"requiredFeatures\":{}",
                json_string_array(&self.required_features)
            ),
            format!(
                "\"requireActiveSubscription\":{}",
                self.require_active_subscription
            ),
        ];
        if let Some(required_scope) = &self.required_scope {
            fields.push(format!("\"requiredScope\":{}", json_string(required_scope)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedContract {
    pub version: String,
    pub request: RequestContract,
    pub responses: Vec<ResponseContract>,
}

impl VersionedContract {
    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"version\":{},\"request\":{},\"responses\":{}}}",
            json_string(&self.version),
            self.request.to_json(),
            json_array(self.responses.iter().map(ResponseContract::to_json))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteContract {
    pub name: String,
    pub operation_name: String,
    pub summary: String,
    pub method: HttpMethod,
    pub path: String,
    pub openapi_path: String,
    pub access: AccessLevel,
    pub request: RequestContract,
    pub responses: Vec<ResponseContract>,
    pub versions: BTreeMap<String, VersionedContract>,
    pub auth: Option<AuthConfig>,
    pub rbac: Option<RbacConfig>,
    pub options: RouteOptions,
}

impl RouteContract {
    #[must_use]
    pub fn get(name: impl Into<String>, summary: impl Into<String>, path: impl AsRef<str>) -> Self {
        Self::new(name, summary, HttpMethod::Get, path)
    }

    #[must_use]
    pub fn post(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Post, path)
    }

    #[must_use]
    pub fn put(name: impl Into<String>, summary: impl Into<String>, path: impl AsRef<str>) -> Self {
        Self::new(name, summary, HttpMethod::Put, path)
    }

    #[must_use]
    pub fn patch(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Patch, path)
    }

    #[must_use]
    pub fn delete(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Delete, path)
    }

    #[must_use]
    pub fn head(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Head, path)
    }

    #[must_use]
    pub fn options(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Options, path)
    }

    #[must_use]
    pub fn trace(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Trace, path)
    }

    #[must_use]
    pub fn checkout(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Checkout, path)
    }

    #[must_use]
    pub fn copy(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Copy, path)
    }

    #[must_use]
    pub fn link(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Link, path)
    }

    #[must_use]
    pub fn lock(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Lock, path)
    }

    #[must_use]
    pub fn m_search(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::MSearch, path)
    }

    #[must_use]
    pub fn merge(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Merge, path)
    }

    #[must_use]
    pub fn mkactivity(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::MkActivity, path)
    }

    #[must_use]
    pub fn mkcol(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::MkCol, path)
    }

    #[must_use]
    pub fn move_resource(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Move, path)
    }

    #[must_use]
    pub fn notify(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Notify, path)
    }

    #[must_use]
    pub fn propfind(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::PropFind, path)
    }

    #[must_use]
    pub fn proppatch(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::PropPatch, path)
    }

    #[must_use]
    pub fn purge(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Purge, path)
    }

    #[must_use]
    pub fn report(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Report, path)
    }

    #[must_use]
    pub fn search(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Search, path)
    }

    #[must_use]
    pub fn subscribe(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Subscribe, path)
    }

    #[must_use]
    pub fn unlink(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Unlink, path)
    }

    #[must_use]
    pub fn unlock(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Unlock, path)
    }

    #[must_use]
    pub fn unsubscribe(
        name: impl Into<String>,
        summary: impl Into<String>,
        path: impl AsRef<str>,
    ) -> Self {
        Self::new(name, summary, HttpMethod::Unsubscribe, path)
    }

    #[must_use]
    pub fn new(
        name: impl Into<String>,
        summary: impl Into<String>,
        method: HttpMethod,
        path: impl AsRef<str>,
    ) -> Self {
        let name = name.into();
        let path = sanitize_path_slashes(path.as_ref());
        Self {
            operation_name: to_pretty_camel_case(&name),
            name,
            summary: summary.into(),
            method,
            openapi_path: open_api_compliant_path(&path),
            path,
            access: AccessLevel::Public,
            request: RequestContract::default(),
            responses: Vec::new(),
            versions: BTreeMap::new(),
            auth: None,
            rbac: None,
            options: RouteOptions::default(),
        }
    }

    #[must_use]
    pub fn with_access(mut self, access: AccessLevel) -> Self {
        self.access = access;
        self
    }

    #[must_use]
    pub fn with_request_body(mut self, body: BodyContract) -> Self {
        self.request.body = Some(body);
        self
    }

    #[must_use]
    pub fn with_param(mut self, field: FieldContract) -> Self {
        self.request.params.push(field);
        self
    }

    #[must_use]
    pub fn with_query(mut self, field: FieldContract) -> Self {
        self.request.query.push(field);
        self
    }

    #[must_use]
    pub fn with_header(mut self, field: FieldContract) -> Self {
        self.request.headers.push(field);
        self
    }

    #[must_use]
    pub fn with_response(mut self, status_code: u16, body: Option<BodyContract>) -> Self {
        self.responses
            .push(ResponseContract::new(status_code, body));
        self
    }

    #[must_use]
    pub fn with_response_header(mut self, status_code: u16, field: FieldContract) -> Self {
        if let Some(response) = self
            .responses
            .iter_mut()
            .find(|response| response.status_code == status_code)
        {
            response.headers.push(field);
        }
        self
    }

    #[must_use]
    pub fn with_options(mut self, options: RouteOptions) -> Self {
        self.options = options;
        self
    }

    #[must_use]
    pub fn with_auth(mut self, auth: AuthConfig) -> Self {
        self.auth = Some(auth);
        self
    }

    #[must_use]
    pub fn with_rbac(mut self, rbac: RbacConfig) -> Self {
        self.rbac = Some(rbac);
        self
    }

    #[must_use]
    pub fn with_version(mut self, version: VersionedContract) -> Self {
        self.versions.insert(version.version.clone(), version);
        self
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!("\"name\":{}", json_string(&self.name)),
            format!("\"operationName\":{}", json_string(&self.operation_name)),
            format!("\"summary\":{}", json_string(&self.summary)),
            format!("\"method\":{}", json_string(self.method.as_str())),
            format!("\"path\":{}", json_string(&self.path)),
            format!("\"openapiPath\":{}", json_string(&self.openapi_path)),
            format!("\"access\":{}", json_string(self.access.as_artifact_str())),
            format!("\"request\":{}", self.request.to_json()),
            format!(
                "\"responses\":{}",
                json_array(self.responses.iter().map(ResponseContract::to_json))
            ),
            format!("\"options\":{}", self.options.to_json()),
        ];
        if !self.versions.is_empty() {
            fields.push(format!(
                "\"versions\":{}",
                json_array(self.versions.values().map(VersionedContract::to_json))
            ));
        }
        if let Some(auth) = &self.auth {
            fields.push(format!("\"auth\":{}", auth.to_json()));
        }
        if let Some(rbac) = &self.rbac {
            fields.push(format!("\"rbac\":{}", rbac.to_json()));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceContract {
    pub name: String,
    pub version: String,
}

impl ServiceContract {
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        format!(
            "{{\"name\":{},\"version\":{}}}",
            json_string(&self.name),
            json_string(&self.version)
        )
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContractRegistry {
    service: Option<ServiceContract>,
    routes: Vec<RouteContract>,
    sockets: BTreeMap<String, String>,
}

impl ContractRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn for_service(service: ServiceContract) -> Self {
        Self {
            service: Some(service),
            routes: Vec::new(),
            sockets: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, route: RouteContract) {
        self.routes.push(route);
    }

    /// Register a raw socket contract for portable service artifact exports.
    /// Pass a `forklaunch_ws::SocketContract` or equivalent serialized metadata.
    /// This records metadata; applications compose their runtime upgrade handlers.
    ///
    /// # Errors
    /// Rejects duplicate/invalid module names and metadata serialization failures.
    #[cfg(feature = "json-schema")]
    pub fn register_socket<T: serde::Serialize + ?Sized>(
        &mut self,
        name: impl Into<String>,
        socket: &T,
    ) -> Result<(), serde_json::Error> {
        let name = name.into();
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(serde::ser::Error::custom("Invalid socket module name"));
        }
        if self.sockets.contains_key(&name) {
            return Err(serde::ser::Error::custom("Duplicate socket module name"));
        }
        let serialized = serde_json::to_string(socket)?;
        self.sockets.insert(name, serialized);
        Ok(())
    }

    #[must_use]
    pub fn socket_contracts(&self) -> &BTreeMap<String, String> {
        &self.sockets
    }

    #[must_use]
    pub fn routes(&self) -> &[RouteContract] {
        &self.routes
    }

    #[must_use]
    pub const fn service(&self) -> Option<&ServiceContract> {
        self.service.as_ref()
    }

    #[must_use]
    pub fn into_artifact(self) -> ContractArtifact {
        ContractArtifact {
            format_version: CONTRACT_ARTIFACT_VERSION.to_owned(),
            generator: "framework-rs".to_owned(),
            service: self.service,
            routes: self.routes,
            sockets: self.sockets,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractArtifact {
    pub format_version: String,
    pub generator: String,
    pub service: Option<ServiceContract>,
    pub routes: Vec<RouteContract>,
    pub sockets: BTreeMap<String, String>,
}

pub type Contract = ContractArtifact;
pub type BodySchema = BodyContract;
pub type HeaderSchema = FieldContract;
pub type ParamSchema = FieldContract;
pub type QuerySchema = FieldContract;
pub type ResponseSchema = ResponseContract;

impl ContractArtifact {
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut fields = vec![
            format!("\"formatVersion\":{}", json_string(&self.format_version)),
            format!("\"generator\":{}", json_string(&self.generator)),
        ];
        if let Some(service) = &self.service {
            fields.push(format!("\"service\":{}", service.to_json()));
        }
        fields.push(format!(
            "\"routes\":{}",
            json_array(self.routes.iter().map(RouteContract::to_json))
        ));
        if !self.sockets.is_empty() {
            fields.push(format!(
                "\"sockets\":{{{}}}",
                self.sockets
                    .iter()
                    .map(|(name, socket)| format!("{}:{socket}", json_string(name)))
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "json-schema")]
    #[test]
    fn socket_registration_exports_raw_metadata_and_rejects_duplicates_without_replacement() {
        let socket = serde_json::json!({"path":"/chat","clientMessages":{"request":{"shape":{"type":"string","const":"water 水"}}}});
        let mut registry = ContractRegistry::for_service(ServiceContract::new("Realtime", "1"));
        assert!(
            !registry
                .clone()
                .into_artifact()
                .to_json()
                .contains("\"sockets\"")
        );
        registry.register_socket("chat", &socket).unwrap();
        assert!(
            registry
                .register_socket("chat", &serde_json::json!({"path":"/replacement"}))
                .is_err()
        );
        for name in ["", "../escape", "Upper", "has-dash"] {
            assert!(registry.register_socket(name, &socket).is_err());
        }
        assert_eq!(registry.socket_contracts().len(), 1);
        let artifact: serde_json::Value =
            serde_json::from_str(&registry.into_artifact().to_json()).unwrap();
        assert_eq!(artifact["sockets"]["chat"], socket);
        assert_eq!(artifact["service"]["name"], "Realtime");
    }

    #[test]
    fn route_contract_normalizes_sdk_and_openapi_names() {
        let route = RouteContract::new(
            "Create User",
            "Create a user",
            HttpMethod::Post,
            "/users/:id",
        )
        .with_response(201, Some(BodyContract::json("User")));

        assert_eq!(route.operation_name, "createUser");
        assert_eq!(route.openapi_path, "/users/{id}");
        assert_eq!(route.responses[0].status_code, 201);
    }

    #[test]
    fn route_builder_helpers_register_request_and_response_fields() {
        let route = RouteContract::get("Get User", "Get a user", "/users/:id")
            .with_param(FieldContract::required("id", "String"))
            .with_query(FieldContract::optional("includePosts", "Boolean"))
            .with_header(FieldContract::optional("x-request-id", "String"))
            .with_response(200, Some(BodyContract::json("User")))
            .with_response_header(200, FieldContract::required("etag", "String"));

        assert_eq!(route.method, HttpMethod::Get);
        assert_eq!(route.request.params[0].name, "id");
        assert_eq!(route.request.query[0].name, "includePosts");
        assert_eq!(route.request.headers[0].name, "x-request-id");
        assert_eq!(route.responses[0].headers[0].name, "etag");
    }

    #[test]
    fn route_contract_serializes_to_language_neutral_artifact() {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("users", "1.0.0"));
        registry.register(
            RouteContract::new("Create User", "Create a user", HttpMethod::Post, "/users")
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, Some(BodyContract::json("User")))
                .with_access(AccessLevel::Protected)
                .with_auth(AuthConfig::Jwt(
                    JwtAuth::new(JwtAuthConfig::JwksUrl {
                        url: "https://auth.example.com/.well-known/jwks.json".to_owned(),
                    })
                    .with_token_prefix("Bearer")
                    .with_header_name("Authorization"),
                )),
        );

        let encoded = registry.into_artifact().to_json();

        assert!(encoded.contains("forklaunch.contract.v1"));
        assert!(encoded.contains("createUser"));
        assert!(encoded.contains("CreateUserBody"));
    }
}
