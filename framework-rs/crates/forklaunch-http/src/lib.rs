#![forbid(unsafe_code)]

pub mod sse;
pub mod versioning;
pub use versioning::{
    BodyParserHooks, MultipartLimits, SelectedVersion, VersionBodyHooks, VersionBodyHooksRegistry,
    VersionFieldHooks, VersionFieldValues, VersionedRoutePlan,
};

use std::{
    collections::BTreeMap,
    fmt::{self, Write as _},
};

use forklaunch_common::{FrameworkError, FrameworkResult, json_string, normalize_route_path};
pub use forklaunch_contract::HttpMethod;
use forklaunch_contract::{ContractRegistry, FieldContract, RouteContract, ValidationMode};
use forklaunch_core::ServiceInfo;

pub type Headers = BTreeMap<String, String>;
pub type Params = BTreeMap<String, String>;
pub type Query = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: Option<String>,
}

impl TraceContext {
    #[must_use]
    pub fn new(trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: None,
        }
    }

    #[must_use]
    pub fn with_span_id(mut self, span_id: impl Into<String>) -> Self {
        self.span_id = Some(span_id.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub correlation_id: String,
    pub idempotency_key: Option<String>,
    pub trace_context: Option<TraceContext>,
}

impl RequestContext {
    #[must_use]
    pub fn new(correlation_id: impl Into<String>) -> Self {
        Self {
            correlation_id: correlation_id.into(),
            idempotency_key: None,
            trace_context: None,
        }
    }

    #[must_use]
    pub fn with_idempotency_key(mut self, idempotency_key: impl Into<String>) -> Self {
        self.idempotency_key = Some(idempotency_key.into());
        self
    }

    #[must_use]
    pub fn with_trace_context(mut self, trace_context: TraceContext) -> Self {
        self.trace_context = Some(trace_context);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Empty,
    Text(String),
    Binary(Vec<u8>),
}

impl Body {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Empty | Self::Binary(_) => None,
        }
    }
}

/// Preserve wire bytes while normalizing empty/text bodies for the route parser.
pub fn normalize_framework_request_body(contract: &RouteContract, request: &mut FrameworkRequest) {
    use forklaunch_contract::BodyKind;
    match contract.parsing_body().map(|body| body.kind) {
        Some(BodyKind::Binary | BodyKind::File | BodyKind::Multipart) => {
            request.body = match std::mem::replace(&mut request.body, Body::Empty) {
                Body::Text(text) => Body::Binary(text.into_bytes()),
                Body::Empty => Body::Binary(Vec::new()),
                bytes @ Body::Binary(_) => bytes,
            };
        }
        Some(BodyKind::Text | BodyKind::UrlEncoded) if request.body.is_empty() => {
            request.body = Body::Text(String::new());
        }
        _ => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkRequest {
    pub context: RequestContext,
    pub method: HttpMethod,
    pub path: String,
    pub params: Params,
    pub query: Query,
    pub headers: Headers,
    pub body: Body,
    /// Verified session claims populated by authentication middleware.
    pub session: Option<serde_json::Value>,
    /// Last matching contract version and all matching declarations.
    pub version: Option<SelectedVersion>,
}

impl FrameworkRequest {
    #[must_use]
    pub fn new(method: HttpMethod, path: impl Into<String>, context: RequestContext) -> Self {
        Self {
            context,
            method,
            path: normalize_route_path(&path.into()),
            params: Params::new(),
            query: Query::new(),
            headers: Headers::new(),
            body: Body::Empty,
            session: None,
            version: None,
        }
    }

    #[must_use]
    pub fn with_query(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(key.into().to_ascii_lowercase(), value.into());
        self
    }

    #[must_use]
    pub fn with_body(mut self, body: Body) -> Self {
        self.body = body;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameworkResponse {
    pub status_code: u16,
    pub headers: Headers,
    pub body: Body,
}

impl FrameworkResponse {
    #[must_use]
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code,
            headers: Headers::new(),
            body: Body::Empty,
        }
    }

    #[must_use]
    pub fn text(status_code: u16, body: impl Into<String>) -> Self {
        Self::new(status_code)
            .with_header("content-type", "text/plain")
            .with_body(Body::Text(body.into()))
    }

    #[must_use]
    pub fn json(status_code: u16, encoded_body: impl Into<String>) -> Self {
        Self::new(status_code)
            .with_header("content-type", "application/json")
            .with_body(Body::Text(encoded_body.into()))
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(key.into().to_ascii_lowercase(), value.into());
        self
    }

    #[must_use]
    pub fn with_body(mut self, body: Body) -> Self {
        self.body = body;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Validation,
    PayloadTooLarge,
    UnsupportedMediaType,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited,
    Internal,
}

impl ErrorKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Validation => "validation_error",
            Self::PayloadTooLarge => "payload_too_large",
            Self::UnsupportedMediaType => "unsupported_media_type",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::RateLimited => "rate_limited",
            Self::Internal => "internal_error",
        }
    }

    #[must_use]
    pub const fn status_code(self) -> u16 {
        match self {
            Self::Validation => 400,
            Self::PayloadTooLarge => 413,
            Self::UnsupportedMediaType => 415,
            Self::Unauthorized => 401,
            Self::Forbidden => 403,
            Self::NotFound => 404,
            Self::RateLimited => 429,
            Self::Internal => 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorResponse {
    pub kind: ErrorKind,
    pub code: String,
    pub message: String,
    pub correlation_id: Option<String>,
}

impl ErrorResponse {
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            code: kind.code().to_owned(),
            message: message.into(),
            correlation_id: None,
        }
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    #[must_use]
    pub fn to_response(&self) -> FrameworkResponse {
        let mut body = format!(
            "{{\"code\":{},\"message\":{}",
            json_string(&self.code),
            json_string(&self.message)
        );
        if let Some(correlation_id) = &self.correlation_id {
            let _ = write!(body, ",\"correlationId\":{}", json_string(correlation_id));
        }
        body.push('}');

        FrameworkResponse::json(self.kind.status_code(), body)
    }

    /// Returns a client-safe copy of this error.
    ///
    /// Internal (`500`) errors have their detail message replaced with a generic
    /// message so implementation details never leak to callers, while the
    /// correlation id is preserved so operators can still tie the response to
    /// server-side logs. Non-internal errors are returned unchanged.
    #[must_use]
    pub fn redacted(&self) -> Self {
        if self.kind == ErrorKind::Internal {
            Self {
                kind: self.kind,
                code: self.code.clone(),
                message: "internal server error".to_owned(),
                correlation_id: self.correlation_id.clone(),
            }
        } else {
            self.clone()
        }
    }
}

impl From<FrameworkError> for ErrorResponse {
    fn from(error: FrameworkError) -> Self {
        Self {
            kind: ErrorKind::Internal,
            code: error.code().to_owned(),
            message: error.message().to_owned(),
            correlation_id: None,
        }
    }
}

/// Wire profile for typed request parser failures; native schema diagnostics are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ParserErrorFormat {
    #[default]
    Json,
    /// Final Express exception response, with private parser details suppressed.
    TypeScriptText,
}

/// Wire profile for framework-detected undocumented response statuses.
/// This does not rewrite arbitrary handler errors or documented responses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ResponseStatusErrorFormat {
    #[default]
    Json,
    /// Redacted TypeScript final exception diagnostic with correlation ID.
    /// Undocumented 400–599 statuses are retained; other statuses become 500.
    /// Native status validation still runs: this does not add default error
    /// schemas or bypass validation for empty responses.
    TypeScriptText,
}

/// Render a parser rejection in the final TypeScript exception-handler format.
#[must_use]
pub fn typescript_parser_error_response(status: u16, correlation_id: &str) -> FrameworkResponse {
    let status = if (400..=599).contains(&status) {
        status
    } else {
        500
    };
    let message = if status >= 500 {
        "Internal server error"
    } else {
        "Request could not be processed"
    };
    FrameworkResponse::text(
        status,
        format!("{message}:\n\nCorrelation id: {correlation_id}"),
    )
    .with_header("content-type", "text/plain; charset=utf-8")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractValidationLocation {
    PathParameter,
    Query,
    Header,
    Body,
    BodyParser,
    BodySize,
    BodyEncoding,
    BodyVerification,
    BodyParserConfiguration,
    ResponseStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractValidationError {
    pub location: ContractValidationLocation,
    pub field: Option<String>,
    pub message: String,
}

impl ContractValidationError {
    /// Parser failures remain fatal even when schema validation is disabled.
    #[must_use]
    pub const fn is_parser_error(&self) -> bool {
        matches!(
            self.location,
            ContractValidationLocation::BodyParser
                | ContractValidationLocation::BodySize
                | ContractValidationLocation::BodyEncoding
                | ContractValidationLocation::BodyVerification
                | ContractValidationLocation::BodyParserConfiguration
        )
    }

    #[must_use]
    pub fn new(location: ContractValidationLocation, message: impl Into<String>) -> Self {
        Self {
            location,
            field: None,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

pub trait TypedHandler<Input, Output> {
    /// # Errors
    ///
    /// Returns an error when the handler cannot produce a valid output for the request context.
    fn handle(&self, input: Input, context: &RequestContext) -> FrameworkResult<Output>;
}

impl<F, Input, Output> TypedHandler<Input, Output> for F
where
    F: Fn(Input, &RequestContext) -> FrameworkResult<Output>,
{
    fn handle(&self, input: Input, context: &RequestContext) -> FrameworkResult<Output> {
        self(input, context)
    }
}

pub trait LocalHandler: Send + Sync {
    /// # Errors
    ///
    /// Returns an error when local route execution fails before an HTTP response is produced.
    fn call(&self, request: FrameworkRequest) -> FrameworkResult<FrameworkResponse>;
}

impl<F> LocalHandler for F
where
    F: Fn(FrameworkRequest) -> FrameworkResult<FrameworkResponse> + Send + Sync,
{
    fn call(&self, request: FrameworkRequest) -> FrameworkResult<FrameworkResponse> {
        self(request)
    }
}

pub trait Middleware: Send + Sync {
    /// # Errors
    ///
    /// Returns an error when the middleware or downstream handler fails before producing a response.
    fn handle(
        &self,
        request: FrameworkRequest,
        next: &dyn Fn(FrameworkRequest) -> FrameworkResult<FrameworkResponse>,
    ) -> FrameworkResult<FrameworkResponse>;
}

#[derive(Default)]
pub struct LocalRouter {
    routes: Vec<LocalRoute>,
}

impl fmt::Debug for LocalRouter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRouter")
            .field("route_count", &self.routes.len())
            .finish()
    }
}

impl LocalRouter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// # Panics
    /// Panics if a version has an invalid embedded JSON Schema.
    pub fn register(
        &mut self,
        contract: RouteContract,
        handler: impl LocalHandler + 'static,
    ) -> &mut Self {
        self.register_with_body_hooks(contract, &VersionBodyHooksRegistry::default(), handler)
    }

    /// # Panics
    /// Panics on invalid schemas or hooks referring to removed versions.
    pub fn register_with_body_hooks(
        &mut self,
        contract: RouteContract,
        hooks: &VersionBodyHooksRegistry,
        handler: impl LocalHandler + 'static,
    ) -> &mut Self {
        let versions = hooks
            .compile(&contract)
            .expect("invalid versioned route schema or hooks");
        self.routes.push(LocalRoute {
            versions,
            contract,
            handler: Box::new(handler),
        });
        self
    }

    /// # Errors
    ///
    /// Returns an error when the matched local handler fails before producing a response.
    pub fn execute(&self, request: FrameworkRequest) -> FrameworkResult<FrameworkResponse> {
        self.execute_with_parser_error_format(request, ParserErrorFormat::Json)
    }

    /// Execute with an opt-in parser rejection profile. Schema errors retain
    /// their native diagnostics and handler failures remain `Err`.
    /// # Errors
    /// Returns an error when the matched handler fails before producing a response.
    pub fn execute_with_parser_error_format(
        &self,
        request: FrameworkRequest,
        parser_error_format: ParserErrorFormat,
    ) -> FrameworkResult<FrameworkResponse> {
        self.execute_with_error_formats(
            request,
            parser_error_format,
            ResponseStatusErrorFormat::Json,
        )
    }

    /// Execute with explicit parser and undocumented-response-status profiles.
    /// Other schema diagnostics and handler failures retain their native behavior.
    /// # Errors
    /// Returns an error when the matched handler fails before producing a response.
    pub fn execute_with_error_formats(
        &self,
        mut request: FrameworkRequest,
        parser_error_format: ParserErrorFormat,
        response_status_error_format: ResponseStatusErrorFormat,
    ) -> FrameworkResult<FrameworkResponse> {
        let explicit_head = request.method == HttpMethod::Head
            && self.routes.iter().any(|route| {
                route.contract.method == HttpMethod::Head
                    && !matches!(
                        match_path_checked(&route.contract.path, &request.path),
                        Ok(None)
                    )
            });
        for route in &self.routes {
            if route.contract.method != request.method
                && !(request.method == HttpMethod::Head
                    && !explicit_head
                    && route.contract.method == HttpMethod::Get)
            {
                continue;
            }

            let params = match match_path_checked(&route.contract.path, &request.path) {
                Ok(params) => params,
                Err(error) => {
                    return Ok(validation_error_response(
                        &[error],
                        &request.context.correlation_id,
                    ));
                }
            };
            if let Some(params) = params {
                request.params = params;
                normalize_framework_request_body(&route.contract, &mut request);
                let selected = route.versions.select(&request);
                let contract = match selected {
                    Ok(Some((contract, version))) => {
                        request.version = Some(version);
                        contract
                    }
                    Err(errors)
                        if route.contract.options.request_validation == ValidationMode::Error
                            || errors.iter().any(ContractValidationError::is_parser_error) =>
                    {
                        return Ok(validation_error_response_with_parser_format(
                            &errors,
                            &request.context.correlation_id,
                            parser_error_format,
                        ));
                    }
                    Ok(None) | Err(_) => route.contract.clone(),
                };
                let request_errors = validate_framework_request(&contract, &request);
                if !request_errors.is_empty() {
                    return Ok(validation_error_response(
                        &request_errors,
                        &request.context.correlation_id,
                    ));
                }
                let correlation_id = request.context.correlation_id.clone();
                let head = request.method == HttpMethod::Head;
                let mut response = route.handler.call(request)?;
                let response_errors = validate_framework_response(&contract, &response);
                if !response_errors.is_empty() {
                    return Ok(match response_status_error_format {
                        ResponseStatusErrorFormat::Json => {
                            validation_error_response(&response_errors, &correlation_id)
                        }
                        ResponseStatusErrorFormat::TypeScriptText => {
                            typescript_parser_error_response(response.status_code, &correlation_id)
                        }
                    });
                }
                if head {
                    response.body = Body::Empty;
                }
                return Ok(response);
            }
        }

        Ok(ErrorResponse::new(ErrorKind::NotFound, "route not found")
            .with_correlation_id(request.context.correlation_id)
            .to_response())
    }
}

#[must_use]
pub fn validate_framework_request(
    contract: &RouteContract,
    request: &FrameworkRequest,
) -> Vec<ContractValidationError> {
    // Version selection has already validated pre-transform values. Rechecking
    // wire presence here would reject defaults and valid absent transform output.
    if contract.options.request_validation != ValidationMode::Error || request.version.is_some() {
        return Vec::new();
    }

    validate_request_fields(
        &contract.request,
        request,
        request
            .version
            .as_ref()
            .is_some_and(|version| version.parsed_body.is_some()),
    )
}

fn validate_request_fields(
    contract: &forklaunch_contract::RequestContract,
    request: &FrameworkRequest,
    has_parsed_body: bool,
) -> Vec<ContractValidationError> {
    let mut errors = Vec::new();
    validate_required_fields(
        ContractValidationLocation::PathParameter,
        "missing required path parameter",
        &contract.params,
        &request.params,
        &mut errors,
    );
    validate_required_fields(
        ContractValidationLocation::Query,
        "missing required query field",
        &contract.query,
        &request.query,
        &mut errors,
    );
    validate_required_headers(&contract.headers, &request.headers, &mut errors);

    errors.extend(validate_request_body(contract, request, has_parsed_body));
    errors
}

fn validate_request_body(
    contract: &forklaunch_contract::RequestContract,
    request: &FrameworkRequest,
    has_parsed_body: bool,
) -> Vec<ContractValidationError> {
    let mut errors = Vec::new();
    if let Some(body) = &contract.body {
        if request.body.is_empty() && !has_parsed_body {
            errors.push(ContractValidationError::new(
                ContractValidationLocation::Body,
                "missing required request body",
            ));
        }
        if let Some(expected_content_type) = body.content_type.as_deref()
            && !request_body_content_type_matches(&request.headers, expected_content_type)
        {
            errors.push(ContractValidationError::new(
                ContractValidationLocation::Body,
                format!("expected request content-type `{expected_content_type}`"),
            ));
        }
    }

    errors
}

fn validate_required_fields(
    location: ContractValidationLocation,
    message: &str,
    fields: &[FieldContract],
    values: &BTreeMap<String, String>,
    errors: &mut Vec<ContractValidationError>,
) {
    for field in fields.iter().filter(|field| field.required) {
        if !values.contains_key(&field.name) {
            errors.push(
                ContractValidationError::new(location, format!("{message} `{}`", field.name))
                    .with_field(field.name.clone()),
            );
        }
    }
}

fn validate_required_headers(
    fields: &[FieldContract],
    headers: &Headers,
    errors: &mut Vec<ContractValidationError>,
) {
    for field in fields.iter().filter(|field| field.required) {
        if header_value(headers, &field.name).is_none() {
            errors.push(
                ContractValidationError::new(
                    ContractValidationLocation::Header,
                    format!("missing required header `{}`", field.name),
                )
                .with_field(field.name.clone()),
            );
        }
    }
}

fn request_body_content_type_matches(headers: &Headers, expected_content_type: &str) -> bool {
    header_value(headers, "content-type").is_some_and(|actual| {
        actual
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case(
                expected_content_type
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim(),
            )
    })
}

fn header_value<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .get(&name.to_ascii_lowercase())
        .or_else(|| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
        .map(String::as_str)
}

#[must_use]
pub fn validate_framework_response(
    contract: &RouteContract,
    response: &FrameworkResponse,
) -> Vec<ContractValidationError> {
    if contract.options.response_validation != ValidationMode::Error
        || contract
            .responses
            .iter()
            .any(|candidate| candidate.status_code == response.status_code)
    {
        return Vec::new();
    }

    vec![ContractValidationError::new(
        ContractValidationLocation::ResponseStatus,
        format!(
            "handler returned undocumented status {}",
            response.status_code
        ),
    )]
}

#[must_use]
pub fn validation_errors_message(errors: &[ContractValidationError]) -> String {
    errors
        .iter()
        .map(|error| error.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

#[must_use]
pub fn validation_error_response(
    errors: &[ContractValidationError],
    correlation_id: &str,
) -> FrameworkResponse {
    validation_error_response_with_parser_format(errors, correlation_id, ParserErrorFormat::Json)
}

/// Render parser errors using the selected profile; schema diagnostics are unchanged.
#[must_use]
pub fn validation_error_response_with_parser_format(
    errors: &[ContractValidationError],
    correlation_id: &str,
    parser_error_format: ParserErrorFormat,
) -> FrameworkResponse {
    let kind = if errors.iter().any(|error| {
        matches!(
            error.location,
            ContractValidationLocation::ResponseStatus
                | ContractValidationLocation::BodyParserConfiguration
        )
    }) {
        ErrorKind::Internal
    } else if errors
        .iter()
        .any(|error| error.location == ContractValidationLocation::BodySize)
    {
        ErrorKind::PayloadTooLarge
    } else if errors
        .iter()
        .any(|error| error.location == ContractValidationLocation::BodyEncoding)
    {
        ErrorKind::UnsupportedMediaType
    } else if errors
        .iter()
        .any(|error| error.location == ContractValidationLocation::BodyVerification)
    {
        ErrorKind::Forbidden
    } else {
        ErrorKind::Validation
    };

    if parser_error_format == ParserErrorFormat::TypeScriptText
        && errors.iter().any(ContractValidationError::is_parser_error)
    {
        return typescript_parser_error_response(kind.status_code(), correlation_id);
    }
    ErrorResponse::new(kind, validation_errors_message(errors))
        .with_correlation_id(correlation_id.to_string())
        .to_response()
}

struct LocalRoute {
    versions: VersionedRoutePlan,
    contract: RouteContract,
    handler: Box<dyn LocalHandler>,
}

#[derive(Debug, Clone)]
pub struct HttpApplication {
    pub service: ServiceInfo,
    pub registry: ContractRegistry,
}

impl HttpApplication {
    #[must_use]
    pub fn new(service: ServiceInfo) -> Self {
        Self {
            service,
            registry: ContractRegistry::new(),
        }
    }

    pub fn register_route(&mut self, route: RouteContract) {
        self.registry.register(route);
    }
}

/// Match a route and decode captured path parameters exactly once.
#[must_use]
pub fn match_path(pattern: &str, path: &str) -> Option<Params> {
    match_path_checked(pattern, path).ok().flatten()
}

/// Match the complete route before decoding captured segments.
///
/// # Errors
/// Returns a path validation error for malformed escapes or non-UTF-8 captures.
pub fn match_path_checked(
    pattern: &str,
    path: &str,
) -> Result<Option<Params>, ContractValidationError> {
    let pattern = normalize_route_path(pattern);
    let path = normalize_route_path(path);
    let pattern_segments = path_segments(&pattern);
    let path_segments = path_segments(&path);
    if pattern_segments.len() != path_segments.len() {
        return Ok(None);
    }
    let mut params = Params::new();
    for (pattern_segment, path_segment) in pattern_segments.iter().zip(path_segments) {
        let name = pattern_segment.strip_prefix(':').or_else(|| {
            pattern_segment
                .strip_prefix('{')
                .and_then(|segment| segment.strip_suffix('}'))
        });
        if let Some(name) = name {
            params.insert(name.to_owned(), path_segment.to_owned());
        } else if *pattern_segment != path_segment {
            return Ok(None);
        }
    }
    for (name, value) in &mut params {
        let invalid = || {
            ContractValidationError::new(
                ContractValidationLocation::PathParameter,
                "invalid path parameter encoding",
            )
            .with_field(name.clone())
        };
        let bytes = value.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%'
                && !bytes
                    .get(index + 1..index + 3)
                    .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(invalid());
            }
        }
        *value = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map_err(|_| invalid())?
            .into_owned();
    }
    Ok(Some(params))
}

fn path_segments(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{BodyContract, FieldContract, RouteContract};

    use super::*;

    #[test]
    fn parser_text_profile_preserves_schema_errors_and_redacts_configuration_details() {
        let schema =
            ContractValidationError::new(ContractValidationLocation::Body, "public schema detail");
        assert_eq!(
            validation_error_response_with_parser_format(
                std::slice::from_ref(&schema),
                "id",
                ParserErrorFormat::TypeScriptText
            ),
            validation_error_response(&[schema], "id")
        );
        let parser = ContractValidationError::new(
            ContractValidationLocation::BodyParserConfiguration,
            "PRIVATE_CONFIG_DETAIL",
        );
        let native = validation_error_response(std::slice::from_ref(&parser), "id");
        assert_eq!(
            native.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        let text = validation_error_response_with_parser_format(
            &[parser],
            "id",
            ParserErrorFormat::TypeScriptText,
        );
        assert_eq!(text.status_code, 500);
        assert_eq!(
            text.body.as_text(),
            Some("Internal server error:\n\nCorrelation id: id")
        );
    }

    #[test]
    fn request_context_tracks_trace_and_idempotency() {
        let context = RequestContext::new("corr-1")
            .with_idempotency_key("idem-1")
            .with_trace_context(TraceContext::new("trace-1").with_span_id("span-1"));

        assert_eq!(context.correlation_id, "corr-1");
        assert_eq!(context.idempotency_key.as_deref(), Some("idem-1"));
        assert_eq!(
            context
                .trace_context
                .as_ref()
                .and_then(|trace| trace.span_id.as_deref()),
            Some("span-1")
        );
    }

    #[test]
    fn response_helpers_set_content_types() {
        let response = FrameworkResponse::json(200, "{}");

        assert_eq!(response.status_code, 200);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(response.body.as_text(), Some("{}"));
    }

    #[test]
    fn typed_handlers_wrap_plain_functions() {
        let handler = |input: String, context: &RequestContext| {
            Ok(format!("{}:{}", context.correlation_id, input))
        };

        let output = handler
            .handle("body".to_owned(), &RequestContext::new("corr-1"))
            .expect("handler should succeed");

        assert_eq!(output, "corr-1:body");
    }

    #[test]
    fn local_router_matches_path_params_and_executes_handler() {
        let mut router = LocalRouter::new();
        router.register(
            RouteContract::get("Get User", "Get a user", "/users/:id").with_response(200, None),
            |request: FrameworkRequest| {
                Ok(FrameworkResponse::text(
                    200,
                    request.params.get("id").cloned().unwrap_or_default(),
                ))
            },
        );

        let response = router
            .execute(FrameworkRequest::new(
                HttpMethod::Get,
                "/users/123",
                RequestContext::new("corr-1"),
            ))
            .expect("local route should execute");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.body.as_text(), Some("123"));
    }

    #[test]
    fn local_router_validates_required_query_and_headers() {
        let mut router = LocalRouter::new();
        router.register(
            RouteContract::get("List Users", "List users", "/users")
                .with_query(FieldContract::required("limit", "Integer"))
                .with_header(FieldContract::required("x-tenant-id", "String"))
                .with_response(200, None),
            |_| Ok(FrameworkResponse::text(200, "ok")),
        );

        let missing = router
            .execute(FrameworkRequest::new(
                HttpMethod::Get,
                "/users",
                RequestContext::new("corr-1"),
            ))
            .expect("validation errors should be represented as responses");

        assert_eq!(missing.status_code, 400);
        assert!(
            missing
                .body
                .as_text()
                .is_some_and(|body| body.contains("limit") && body.contains("x-tenant-id"))
        );

        let valid = router
            .execute(
                FrameworkRequest::new(HttpMethod::Get, "/users", RequestContext::new("corr-1"))
                    .with_query("limit", "10")
                    .with_header("X-Tenant-Id", "tenant-1"),
            )
            .expect("valid request should execute");

        assert_eq!(valid.status_code, 200);
    }

    #[test]
    fn framework_request_validation_reports_structured_errors() {
        let contract = RouteContract::get("Get User", "Get a user", "/users/:id")
            .with_param(FieldContract::required("id", "String"))
            .with_query(FieldContract::required("includePosts", "Boolean"))
            .with_header(FieldContract::required("x-tenant-id", "String"))
            .with_response(200, None);
        let request =
            FrameworkRequest::new(HttpMethod::Get, "/users/123", RequestContext::new("corr-1"));

        let errors = validate_framework_request(&contract, &request);

        assert_eq!(errors.len(), 3);
        assert!(errors.iter().any(|error| {
            error.location == ContractValidationLocation::PathParameter
                && error.field.as_deref() == Some("id")
        }));
        assert!(errors.iter().any(|error| {
            error.location == ContractValidationLocation::Query
                && error.field.as_deref() == Some("includePosts")
        }));
        assert!(errors.iter().any(|error| {
            error.location == ContractValidationLocation::Header
                && error.field.as_deref() == Some("x-tenant-id")
        }));
    }

    #[test]
    fn local_router_validates_required_body_and_content_type() {
        let mut router = LocalRouter::new();
        router.register(
            RouteContract::post("Create User", "Create a user", "/users")
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, None),
            |_| Ok(FrameworkResponse::text(201, "created")),
        );

        let missing = router
            .execute(FrameworkRequest::new(
                HttpMethod::Post,
                "/users",
                RequestContext::new("corr-1"),
            ))
            .expect("validation errors should be represented as responses");

        assert_eq!(missing.status_code, 400);
        assert!(
            missing
                .body
                .as_text()
                .is_some_and(|body| body.contains("request body"))
        );

        let valid = router
            .execute(
                FrameworkRequest::new(HttpMethod::Post, "/users", RequestContext::new("corr-1"))
                    .with_header("content-type", "application/json; charset=utf-8")
                    .with_body(Body::Text("{}".to_string())),
            )
            .expect("valid request should execute");

        assert_eq!(valid.status_code, 201);
    }

    #[test]
    fn media_validation_compares_types_on_both_sides_without_charset_or_boundary_parameters() {
        use forklaunch_contract::{BodyKind, SchemaRef};
        for (expected, actual, valid) in [
            (
                "text/plain; charset=utf-8",
                "TEXT/PLAIN; charset=UTF-8",
                true,
            ),
            ("text/plain; charset=utf-8", "text/plain", true),
            (
                "multipart/form-data",
                "multipart/form-data; boundary=actual-boundary",
                true,
            ),
            (
                "application/vnd.example+json; charset=utf-8",
                "application/vnd.example+json",
                true,
            ),
            ("application/json", "application/problem+json", false),
            ("text/plain; charset=utf-8", "application/json", false),
        ] {
            let contract = RouteContract::post("Upload", "Upload", "/upload").with_request_body(
                BodyContract {
                    kind: BodyKind::Text,
                    schema: SchemaRef::named("Body"),
                    content_type: Some(expected.into()),
                },
            );
            let request =
                FrameworkRequest::new(HttpMethod::Post, "/upload", RequestContext::new("test"))
                    .with_header("content-type", actual)
                    .with_body(Body::Text("payload".into()));
            assert_eq!(
                validate_framework_request(&contract, &request).is_empty(),
                valid,
                "expected {expected}, actual {actual}"
            );
        }
    }

    #[test]
    fn local_router_validates_response_status_codes() {
        let mut router = LocalRouter::new();
        router.register(
            RouteContract::get("List Users", "List users", "/users").with_response(200, None),
            |_| Ok(FrameworkResponse::text(201, "created")),
        );

        let response = router
            .execute(FrameworkRequest::new(
                HttpMethod::Get,
                "/users",
                RequestContext::new("corr-1"),
            ))
            .expect("response validation errors should be represented as responses");

        assert_eq!(response.status_code, 500);
        assert!(
            response
                .body
                .as_text()
                .is_some_and(|body| body.contains("undocumented status 201"))
        );
    }

    #[test]
    fn framework_response_validation_reports_structured_status_error() {
        let contract =
            RouteContract::get("List Users", "List users", "/users").with_response(200, None);
        let response = FrameworkResponse::new(202);

        let errors = validate_framework_response(&contract, &response);

        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].location,
            ContractValidationLocation::ResponseStatus
        );
        assert_eq!(
            validation_errors_message(&errors),
            "handler returned undocumented status 202"
        );
    }

    #[test]
    fn local_router_returns_not_found_response() {
        let router = LocalRouter::new();

        let response = router
            .execute(FrameworkRequest::new(
                HttpMethod::Get,
                "/missing",
                RequestContext::new("corr-1"),
            ))
            .expect("not found should be represented as a response");

        assert_eq!(response.status_code, 404);
        assert!(
            response
                .body
                .as_text()
                .is_some_and(|body| body.contains("corr-1"))
        );
    }

    #[test]
    fn path_matching_supports_colon_and_openapi_params() {
        let colon_params = match_path("/users/:id", "/users/123").expect("path should match");
        let openapi_params = match_path("/users/{id}/posts/{postId}", "/users/123/posts/456")
            .expect("path should match");

        assert_eq!(colon_params.get("id").map(String::as_str), Some("123"));
        assert_eq!(
            openapi_params.get("postId").map(String::as_str),
            Some("456")
        );
        assert!(match_path("/users/:id", "/accounts/123").is_none());
    }

    #[test]
    fn redacts_internal_errors_but_preserves_correlation_id() {
        let internal = ErrorResponse::new(ErrorKind::Internal, "database password invalid")
            .with_correlation_id("corr-7")
            .redacted();
        assert_eq!(internal.message, "internal server error");
        assert_eq!(internal.correlation_id.as_deref(), Some("corr-7"));

        let validation = ErrorResponse::new(ErrorKind::Validation, "name is required").redacted();
        assert_eq!(validation.message, "name is required");
    }
}

#[cfg(test)]
mod head_dispatch_tests {
    use super::*;
    #[test]
    fn local_head_retains_wire_method_and_explicit_head_takes_precedence() {
        let mut router = LocalRouter::new();
        router.register(
            RouteContract::get("Read", "Read", "/read").with_response(200, None),
            |request: FrameworkRequest| {
                assert_eq!(
                    request.method.as_str(),
                    request.headers["x-expected-method"]
                );
                Ok(FrameworkResponse::text(200, "get body").with_header("x-selected", "get"))
            },
        );
        let head = || {
            FrameworkRequest::new(HttpMethod::Head, "/read", RequestContext::new("head-test"))
                .with_header("x-expected-method", "HEAD")
        };
        let fallback = router.execute(head()).unwrap();
        assert_eq!(fallback.status_code, 200);
        assert_eq!(fallback.headers["x-selected"], "get");
        assert_eq!(fallback.body, Body::Empty);
        router.register(
            RouteContract::head("Head", "Head", "/read").with_response(204, None),
            |request: FrameworkRequest| {
                assert_eq!(request.method, HttpMethod::Head);
                Ok(FrameworkResponse::new(204).with_header("x-selected", "head"))
            },
        );
        let explicit = router.execute(head()).unwrap();
        assert_eq!(explicit.status_code, 204);
        assert_eq!(explicit.headers["x-selected"], "head");
        assert_eq!(explicit.body, Body::Empty);
        let get = router
            .execute(
                FrameworkRequest::new(HttpMethod::Get, "/read", RequestContext::new("get-test"))
                    .with_header("x-expected-method", "GET"),
            )
            .unwrap();
        assert_eq!(get.body, Body::Text("get body".into()));
    }
}

#[cfg(test)]
mod path_encoding_tests {
    use super::*;
    #[test]
    fn captures_decode_once_without_changing_route_segments() {
        for (wire, decoded) in [
            ("%20a%20", " a "),
            ("a%2Fb", "a/b"),
            ("%252F", "%2F"),
            ("a+b", "a+b"),
            ("%E2%98%83", "☃"),
            ("%3F%23%25", "?#%"),
        ] {
            let params = match_path_checked("/users/:id", &format!("/users/{wire}"))
                .unwrap()
                .unwrap();
            assert_eq!(params["id"], decoded);
        }
        assert!(
            match_path_checked("/users/:id", "/users/a/b")
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn local_router_rejects_encoding_errors_even_without_schema_validation() {
        let mut route = RouteContract::get("Read", "Read", "/users/:id").with_response(204, None);
        route.options.request_validation = ValidationMode::None;
        let mut router = LocalRouter::new();
        router.register(route, |request: FrameworkRequest| {
            assert_eq!(request.path, "/users/%252F");
            assert_eq!(request.params["id"], "%2F");
            Ok(FrameworkResponse::new(204))
        });
        for (path, status) in [
            ("/users/%252F", 204),
            ("/users/%FF", 400),
            ("/users/%", 400),
        ] {
            let response = router
                .execute(FrameworkRequest::new(
                    HttpMethod::Get,
                    path,
                    RequestContext::new("encoding"),
                ))
                .unwrap();
            assert_eq!(response.status_code, status);
        }
    }

    #[test]
    fn invalid_captures_fail_only_after_route_shape_matches() {
        for wire in ["%", "%2", "%GG", "%FF", "%C0%AF", "%E2%98"] {
            let error = match_path_checked("/users/{id}", &format!("/users/{wire}")).unwrap_err();
            assert_eq!(error.location, ContractValidationLocation::PathParameter);
            assert_eq!(error.field.as_deref(), Some("id"));
            assert!(
                match_path_checked("/users/{id}/posts", &format!("/users/{wire}/other"))
                    .unwrap()
                    .is_none()
            );
        }
    }
}
