//! Code-set provider DTOs and contracts shared with the TypeScript CAC module.
#![forbid(unsafe_code)]
use forklaunch_blueprint_core::BlueprintResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProcedureCodeLookupDto {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProcedureCodeDto {
    pub code: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CodeSetType {
    Mock,
    Cpt,
}

/// Reports provider licensing metadata. A descriptor does not itself grant a
/// license or contain licensed codes; consumers must configure their provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CodeSetDescriptorDto {
    pub code_set_type: CodeSetType,
    pub licensed: bool,
}

pub trait CodeSetProvider: Send + Sync {
    /// # Errors
    /// Returns provider/storage failures; unknown codes return `Ok(None)`.
    fn lookup_procedure_code(
        &self,
        lookup: ProcedureCodeLookupDto,
    ) -> impl Future<Output = BlueprintResult<Option<ProcedureCodeDto>>> + Send;
    fn describe(&self) -> CodeSetDescriptorDto;
}

/// Internal HMAC-protected code-set routes. This registry describes the lookup
/// provider surface only; the generated CAC module also requires protected
/// compliance export/erasure routes backed by its persistence service.
pub mod contracts {
    use super::{CodeSetDescriptorDto, ProcedureCodeDto};
    use forklaunch_blueprint_core::contract::HmacAuth;
    use forklaunch_blueprint_core::contract::{
        AccessLevel, AuthConfig, BodyContract, BodyKind, ContractRegistry, FieldContract,
        RouteContract, SchemaRef, ServiceContract,
    };

    fn json_body<T: schemars::JsonSchema>(name: &str) -> BodyContract {
        let mut body = BodyContract::json(name);
        let mut settings = schemars::r#gen::SchemaSettings::draft07();
        settings.inline_subschemas = true;
        body.schema.json_schema = Some(
            serde_json::to_string(&settings.into_generator().into_root_schema_for::<T>())
                .expect("JSON Schema serializes"),
        );
        body
    }

    #[must_use]
    pub fn registry() -> ContractRegistry {
        let mut registry = ContractRegistry::for_service(ServiceContract::new("cac", "0.1.0"));
        registry.register(
            RouteContract::get(
                "Describe Code Set",
                "Reports which procedure code-set provider is active (mock vs. licensed CPT)",
                "/codeSet",
            )
            .with_access(AccessLevel::Internal)
            .with_auth(AuthConfig::Hmac(
                HmacAuth::new().with_header_name("authorization"),
            ))
            .with_response(
                200,
                Some(json_body::<CodeSetDescriptorDto>("CodeSetDescriptor")),
            ),
        );
        registry.register(
            RouteContract::get(
                "Lookup Procedure Code",
                "Looks up a procedure code against the currently active code-set provider",
                "/codeSet/:code",
            )
            .with_access(AccessLevel::Internal)
            .with_auth(AuthConfig::Hmac(
                HmacAuth::new().with_header_name("authorization"),
            ))
            .with_param(FieldContract::required("code", "String"))
            .with_response(200, Some(json_body::<ProcedureCodeDto>("ProcedureCode")))
            .with_response(
                404,
                Some(BodyContract {
                    kind: BodyKind::Text,
                    schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
                    content_type: Some("text/plain".to_owned()),
                }),
            ),
        );
        registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_names_and_route_security_match_the_typescript_provider() {
        let value = serde_json::to_value(CodeSetDescriptorDto {
            code_set_type: CodeSetType::Mock,
            licensed: false,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"codeSetType":"mock","licensed":false})
        );
        let registry = contracts::registry();
        assert_eq!(registry.routes().len(), 2);
        for route in registry.routes() {
            assert_eq!(
                route.access,
                forklaunch_blueprint_core::contract::AccessLevel::Internal
            );
            assert!(matches!(
                route.auth,
                Some(forklaunch_blueprint_core::contract::AuthConfig::Hmac(_))
            ));
            let schema: serde_json::Value = serde_json::from_str(
                route.responses[0]
                    .body
                    .as_ref()
                    .unwrap()
                    .schema
                    .json_schema
                    .as_ref()
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(schema["type"], "object");
        }
    }
}
