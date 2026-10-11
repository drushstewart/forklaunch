//! Checkout tax and shipping calculations, expressed in integral USD cents.
use crate::{ShippingAddressDto, TaxLineDto};
use forklaunch_blueprint_core::BlueprintResult;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::future::Future;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaxCalculation {
    pub line_item_cents: Vec<i64>,
    pub shipping_address: ShippingAddressDto,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaxCalculationResult {
    pub tax_cents: i64,
    pub breakdown: Vec<TaxLineDto>,
    pub estimated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShippingCalculation {
    pub shipping_address: ShippingAddressDto,
    pub subtotal_cents: i64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShippingCalculationResult {
    pub shipping_cents: i64,
}
pub trait TaxService: Send + Sync {
    fn calculate(
        &self,
        request: TaxCalculation,
    ) -> impl Future<Output = BlueprintResult<TaxCalculationResult>> + Send;
}
pub trait ShippingService: Send + Sync {
    fn calculate(
        &self,
        request: ShippingCalculation,
    ) -> impl Future<Output = BlueprintResult<ShippingCalculationResult>> + Send;
}
