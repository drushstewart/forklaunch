//! HTTP module request/response DTOs distinct from persistence service inputs.
use crate::{OrderStatus, PaymentDto, optional};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PaymentProvider {
    Stripe,
    Paypal,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateProviderPayment {
    pub order_id: String,
    pub amount_cents: i64,
    pub currency: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    #[schemars(with = "PaymentProvider")]
    pub provider: Option<PaymentProvider>,
}
/// Creation can include an ephemeral Stripe secret; persistent reads cannot.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreatedPayment {
    #[serde(flatten)]
    pub payment: PaymentDto,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    #[schemars(with = "String")]
    pub client_secret: Option<String>,
}
impl std::fmt::Debug for CreatedPayment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CreatedPayment")
            .field("payment", &self.payment)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrderTransition {
    pub to: OrderStatus,
}
