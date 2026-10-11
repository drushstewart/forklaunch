use super::models::RustTypeAliasTemplate;
use ramhorns::Content;

#[derive(Content, Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct SdkResponseVariant {
    pub response_type: String,
    pub response_body_type: String,
    pub rust_response_type: String,
    pub status_code: u16,
    pub status_variant: String,
    pub empty_response: bool,
    pub json_response: bool,
    pub text_response: bool,
    pub binary_response: bool,
    pub streaming_response: bool,
    pub response_kind: String,
    pub content_type: String,
}

#[derive(Content)]
#[allow(clippy::struct_excessive_bools)]
pub struct RustSdkTemplate {
    pub model_source: String,
    pub has_operations: bool,
    pub has_multipart: bool,
    pub has_sse: bool,
    pub sse_source: String,
    pub has_serde: bool,
    pub client_name: String,
    pub operations: Vec<RustOperationTemplate>,
}

#[derive(Content)]
pub struct RustOperationTemplate {
    pub request_type: String,
    pub responses_type: String,
    pub responses: Vec<SdkResponseVariant>,
    pub execute_method_name: String,
    pub method_name: String,
    pub http_method: String,
    pub path: String,
    pub binary_request: bool,
    pub multipart_request: bool,
    pub multipart_field: String,
    pub streaming_response: bool,
    pub request_content_type: String,
}

#[derive(Content)]
pub struct TypeScriptSdkTemplate {
    pub schema_models: Vec<RustTypeAliasTemplate>,
    pub form_stringify_source: String,
    pub sse_source: String,
    pub operation_types: Vec<TypeScriptOperationTypeTemplate>,
    pub services: Vec<TypeScriptServiceTemplate>,
    pub aliases: Vec<TypeScriptAliasTemplate>,
}

#[derive(Content)]
pub struct TypeScriptOperationTypeTemplate {
    pub request_type: String,
    pub responses_type: String,
    pub request_params_type: String,
    pub request_query_type: String,
    pub request_body_type: String,
    pub request_headers_type: String,
    pub responses: Vec<SdkResponseVariant>,
}

#[derive(Content)]
pub struct TypeScriptServiceTemplate {
    pub service_key: String,
    pub routers: Vec<TypeScriptRouterTemplate>,
    pub last: bool,
}

#[derive(Content)]
pub struct TypeScriptRouterTemplate {
    pub router_key: String,
    pub operations: Vec<TypeScriptOperationTemplate>,
    pub last: bool,
}

#[derive(Content)]
pub struct TypeScriptOperationTemplate {
    pub operation_key: String,
    pub request_type: String,
    pub responses_type: String,
    pub optional_input: bool,
    pub service_name: String,
    pub router_name: String,
    pub operation_name: String,
    pub method: String,
    pub path: String,
    pub request_kind: String,
    pub request_content_type: String,
    pub responses: Vec<SdkResponseVariant>,
    pub last: bool,
}

#[derive(Content)]
pub struct TypeScriptAliasTemplate {
    pub alias: String,
    pub service_key: String,
}
