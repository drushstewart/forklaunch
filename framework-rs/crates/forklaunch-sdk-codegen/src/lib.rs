#![forbid(unsafe_code)]

pub use forklaunch_contract::BodyKind;
use forklaunch_contract::{ContractRegistry, HttpMethod};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkTarget {
    TypeScript,
    Rust,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkGenerationPlan {
    pub target: SdkTarget,
    pub operations: Vec<SdkOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkOperation {
    pub name: String,
    pub method: HttpMethod,
    pub path: String,
    pub request_params: Vec<SdkField>,
    pub request_query: Vec<SdkField>,
    pub request_headers: Vec<SdkField>,
    pub request_body: Option<String>,
    pub request_body_kind: Option<BodyKind>,
    pub request_body_content_type: Option<String>,
    pub responses: Vec<SdkResponse>,
    pub response_status_codes: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkField {
    pub name: String,
    pub schema: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkResponse {
    pub status_code: u16,
    pub body: Option<String>,
    pub headers: Vec<SdkField>,
}

impl SdkGenerationPlan {
    #[must_use]
    pub fn from_registry(target: SdkTarget, registry: &ContractRegistry) -> Self {
        Self {
            target,
            operations: registry
                .routes()
                .iter()
                .map(|route| SdkOperation {
                    name: route.operation_name.clone(),
                    method: route.method,
                    path: route.path.clone(),
                    request_params: route.request.params.iter().map(SdkField::from).collect(),
                    request_query: route.request.query.iter().map(SdkField::from).collect(),
                    request_headers: route.request.headers.iter().map(SdkField::from).collect(),
                    request_body: route
                        .request
                        .body
                        .as_ref()
                        .map(|body| body.schema.name.clone()),
                    request_body_kind: route.request.body.as_ref().map(|body| body.kind),
                    request_body_content_type: route
                        .request
                        .body
                        .as_ref()
                        .and_then(|body| body.content_type.clone()),
                    responses: route
                        .responses
                        .iter()
                        .map(|response| SdkResponse {
                            status_code: response.status_code,
                            body: response.body.as_ref().map(|body| body.schema.name.clone()),
                            headers: response.headers.iter().map(SdkField::from).collect(),
                        })
                        .collect(),
                    response_status_codes: route
                        .responses
                        .iter()
                        .map(|response| response.status_code)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl From<&forklaunch_contract::FieldContract> for SdkField {
    fn from(field: &forklaunch_contract::FieldContract) -> Self {
        Self {
            name: field.name.clone(),
            schema: field.schema.name.clone(),
            required: field.required,
        }
    }
}

#[cfg(test)]
mod tests {
    use forklaunch_contract::{BodyContract, FieldContract, HttpMethod, RouteContract};

    use super::*;

    #[test]
    fn sdk_generation_plan_preserves_live_typed_operation_shape() {
        let mut registry = ContractRegistry::new();
        registry.register(
            RouteContract::new("Create User", "Create a user", HttpMethod::Post, "/users")
                .with_query(FieldContract::optional("dryRun", "Boolean"))
                .with_header(FieldContract::required("authorization", "String"))
                .with_request_body(BodyContract::json("CreateUserBody"))
                .with_response(201, Some(BodyContract::json("User"))),
        );

        let plan = SdkGenerationPlan::from_registry(SdkTarget::TypeScript, &registry);

        assert_eq!(plan.operations[0].name, "createUser");
        assert_eq!(
            plan.operations[0].request_body.as_deref(),
            Some("CreateUserBody")
        );
        assert_eq!(
            plan.operations[0].request_body_content_type.as_deref(),
            Some("application/json")
        );
        assert_eq!(plan.operations[0].request_query[0].name, "dryRun");
        assert!(!plan.operations[0].request_query[0].required);
        assert_eq!(plan.operations[0].request_headers[0].name, "authorization");
        assert!(plan.operations[0].request_headers[0].required);
        assert_eq!(
            plan.operations[0].responses[0].body.as_deref(),
            Some("User")
        );
        assert_eq!(plan.operations[0].response_status_codes, vec![201]);
    }
}
