//! SMS DTOs shared with the TypeScript messaging blueprints.
#![forbid(unsafe_code)]
use forklaunch_blueprint_core::BlueprintResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::future::Future;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SmsStatus {
    Queued,
    Sent,
    Delivered,
    Failed,
    Undelivered,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SendSmsDto {
    pub to: String,
    pub body: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_metadata"
    )]
    #[schemars(with = "Map<String, Value>")]
    pub metadata: Option<Map<String, Value>>,
}

fn optional_metadata<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Map<String, Value>>, D::Error> {
    Map::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsRecordDto {
    pub id: String,
    pub to: String,
    pub body: String,
    pub status: SmsStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// ISO 8601 timestamps, matching serialized TypeScript `Date` values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

pub trait SmsService: Send + Sync {
    /// # Errors
    /// Returns a provider or persistence error. Dispatch is never retried implicitly.
    fn send_sms(
        &self,
        request: SendSmsDto,
    ) -> impl Future<Output = BlueprintResult<SmsRecordDto>> + Send;
    /// # Errors
    /// Missing records return `sms_not_found`.
    fn get_sms_record(
        &self,
        id: &str,
    ) -> impl Future<Output = BlueprintResult<SmsRecordDto>> + Send;
}

/// Storage updates callbacks atomically by provider ID. Implementations must
/// bind their tenant from trusted application configuration, never request data.
pub trait SmsStore: Send + Sync {
    /// # Errors
    /// Returns a sanitized storage error; duplicate IDs must not overwrite rows.
    fn insert(
        &self,
        record: SmsRecordDto,
    ) -> impl Future<Output = BlueprintResult<SmsRecordDto>> + Send;
    /// # Errors
    /// Returns `sms_not_found` for missing or other-tenant records.
    fn get(&self, id: &str) -> impl Future<Output = BlueprintResult<SmsRecordDto>> + Send;
    /// # Errors
    /// Returns `sms_not_found` for an unknown provider ID.
    fn update_delivery(
        &self,
        provider_id: &str,
        status: SmsStatus,
        error: Option<String>,
    ) -> impl Future<Output = BlueprintResult<SmsRecordDto>> + Send;
}

pub mod contracts {
    use super::{SendSmsDto, SmsRecordDto};
    use forklaunch_blueprint_core::contract::{
        AccessLevel, AuthConfig, BodyContract, ContractRegistry, FieldContract, HmacAuth,
        RouteContract, ServiceContract,
    };
    fn body<T: schemars::JsonSchema>(name: &str) -> BodyContract {
        let mut body = BodyContract::json(name);
        let mut settings = schemars::r#gen::SchemaSettings::draft07();
        settings.inline_subschemas = true;
        body.schema.json_schema = Some(
            serde_json::to_string(&settings.into_generator().into_root_schema_for::<T>())
                .expect("JSON Schema serializes"),
        );
        body
    }
    /// SMS application routes shared by base and Twilio modules. Authentication
    /// and body serialization match the TS HMAC controllers.
    #[must_use]
    pub fn registry() -> ContractRegistry {
        let mut registry =
            ContractRegistry::for_service(ServiceContract::new("messaging", "0.1.0"));
        registry.register(
            RouteContract::post("Send Sms", "Sends and persists an SMS message", "/sms/")
                .with_access(AccessLevel::Internal)
                .with_auth(AuthConfig::Hmac(
                    HmacAuth::new().with_header_name("authorization"),
                ))
                .with_request_body(body::<SendSmsDto>("SendSms"))
                .with_response(200, Some(body::<SmsRecordDto>("SmsRecord"))),
        );
        registry.register(
            RouteContract::get(
                "Get Sms Record",
                "Returns a persisted SMS record",
                "/sms/:id",
            )
            .with_access(AccessLevel::Internal)
            .with_auth(AuthConfig::Hmac(
                HmacAuth::new().with_header_name("authorization"),
            ))
            .with_param(FieldContract::required("id", "String"))
            .with_response(200, Some(body::<SmsRecordDto>("SmsRecord")))
            .with_response(404, Some(text())),
        );
        registry
    }
    fn text() -> BodyContract {
        use forklaunch_blueprint_core::contract::{BodyKind, SchemaRef};
        BodyContract {
            kind: BodyKind::Text,
            schema: SchemaRef::named("String").with_json_schema(r#"{"type":"string"}"#),
            content_type: Some("text/plain".into()),
        }
    }
    /// Complete base or Twilio module surface. The Twilio HMAC uses its own
    /// signature algorithm and explicitly configured external callback URL.
    #[must_use]
    pub fn module_registry(twilio: bool) -> ContractRegistry {
        use super::{SmsErasureDto, SmsExportDto};
        use forklaunch_blueprint_core::contract::{BodyKind, JwtAuthConfig, RbacConfig, SchemaRef};
        let mut registry = registry();
        for (route, response) in [
            (
                RouteContract::get(
                    "Export User Data",
                    "Exports SMS PII for a phone-number subject",
                    "/compliance/export/:userId",
                ),
                body::<SmsExportDto>("SmsExport"),
            ),
            (
                RouteContract::delete(
                    "Erase User Data",
                    "Erases SMS records for a phone-number subject",
                    "/compliance/erase/:userId",
                ),
                body::<SmsErasureDto>("SmsErasure"),
            ),
        ] {
            registry.register(
                route
                    .with_access(AccessLevel::Protected)
                    .with_auth(AuthConfig::jwt(JwtAuthConfig::JwksUrl {
                        url: "${JWKS_PUBLIC_KEY_URL}".into(),
                    }))
                    .with_rbac(RbacConfig {
                        allowed_roles: vec!["system".into()],
                        ..RbacConfig::default()
                    })
                    .with_param(FieldContract::required("userId", "String"))
                    .with_response(200, Some(response))
                    .with_response(404, Some(text())),
            );
        }
        registry.register(RouteContract::get("Get Health","Reports service health","/health")
            .with_response(200,Some(BodyContract {kind:BodyKind::Json,schema:SchemaRef::named("MessagingHealth").with_json_schema(r#"{"type":"object","required":["service","status"],"properties":{"service":{"type":"string"},"status":{"type":"string"}}}"#),content_type:Some("application/json".into())})));
        if twilio {
            registry.register(RouteContract::post("Handle Status Callback","Authenticates and processes a Twilio status callback","/webhook/")
                .with_auth(AuthConfig::Hmac(HmacAuth::new().with_header_name("x-twilio-signature")))
                .with_request_body(BodyContract {kind:BodyKind::UrlEncoded,schema:SchemaRef::named("TwilioStatusCallback").with_json_schema(r#"{"type":"object","required":["MessageSid","MessageStatus"],"properties":{"MessageSid":{"type":"string"},"MessageStatus":{"type":"string"},"ErrorMessage":{"type":"string"}},"additionalProperties":true}"#),content_type:Some("application/x-www-form-urlencoded".into())})
                .with_response(200,Some(text())).with_response(404,Some(text())));
        }
        registry
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmsPiiRecordDto {
    pub id: String,
    pub to: String,
    pub body: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComplianceFailureDto {
    pub entity_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsExportDto {
    pub user_id: String,
    pub entities: std::collections::BTreeMap<String, Vec<SmsPiiRecordDto>>,
    pub failures: Vec<ComplianceFailureDto>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmsErasureDto {
    pub entities_affected: Vec<String>,
    pub records_deleted: u64,
    pub failures: Vec<ComplianceFailureDto>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_metadata_preserves_typescript_omission_without_accepting_null() {
        assert!(
            serde_json::from_value::<SendSmsDto>(
                serde_json::json!({"to":"","body":"","metadata":null})
            )
            .is_err()
        );
        assert_eq!(
            serde_json::from_value::<SendSmsDto>(serde_json::json!({"to":"","body":""}))
                .unwrap()
                .metadata,
            None
        );
        let schema = serde_json::to_value(schemars::schema_for!(SendSmsDto)).unwrap();
        assert_eq!(schema["properties"]["metadata"]["type"], "object");
        assert!(
            !schema["required"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("metadata"))
        );
    }
}
