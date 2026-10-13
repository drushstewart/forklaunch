//! Generic retention integration for configured ecommerce Order metadata.
//! No default retention period is imposed. Applications resolve calendar cutoffs
//! and select a policy explicitly through the framework retention API.
use forklaunch_core::{
    common::{FrameworkError, FrameworkResult},
    persistence::{
        TenantContext,
        compliance::{ComplianceField, ComplianceLevel, ComplianceMetadata},
        retention::RetentionRequest,
    },
};

/// The configured shipping address is classified PII but non-nullable. Therefore
/// anonymization records its completion without clearing that required address,
/// matching TypeScript. A delete policy removes the entire Order instead.
pub struct OrderCompliance;
impl ComplianceMetadata for OrderCompliance {
    const ENTITY: &'static str = "Order";
    const FIELDS: &'static [ComplianceField] = &[
        ComplianceField {
            rust_name: "id",
            storage_name: "id",
            level: ComplianceLevel::None,
            nullable: false,
        },
        ComplianceField {
            rust_name: "customer_id",
            storage_name: "customerId",
            level: ComplianceLevel::None,
            nullable: true,
        },
        ComplianceField {
            rust_name: "shipping_address",
            storage_name: "shippingAddress",
            level: ComplianceLevel::Pii,
            nullable: false,
        },
        ComplianceField {
            rust_name: "created_at",
            storage_name: "createdAt",
            level: ComplianceLevel::None,
            nullable: false,
        },
        ComplianceField {
            rust_name: "updated_at",
            storage_name: "updatedAt",
            level: ComplianceLevel::None,
            nullable: false,
        },
        ComplianceField {
            rust_name: "retention_anonymized_at",
            storage_name: "retentionAnonymizedAt",
            level: ComplianceLevel::None,
            nullable: true,
        },
    ];
}
pub(crate) fn validate(
    request: RetentionRequest<'_>,
    tenant: Option<&TenantContext>,
) -> FrameworkResult<()> {
    if tenant.is_none_or(|tenant| tenant.id() != request.tenant.id())
        || request.policy.entity() != "Order"
        || !request.policy.anonymize_fields().is_empty()
        || !(1..=10_000).contains(&request.batch_size)
        || request.policy.cutoff() > request.now
    {
        return Err(FrameworkError::new(
            "ecommerce_retention_invalid",
            "Invalid ecommerce retention scope or policy",
        ));
    }
    Ok(())
}
pub(crate) fn storage_error<T>(_: T) -> FrameworkError {
    FrameworkError::new(
        "ecommerce_retention_storage_error",
        "Unable to complete ecommerce retention operation",
    )
}
