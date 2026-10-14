//! Stripe provider orchestration over tenant-bound billing persistence.
//! `provider_fields` carries Stripe request fields on input and provider objects
//! on output. Generated Stripe contracts must expose the TS `stripeFields` name.
//! Provider mutations precede local persistence, matching the TS wrappers. A
//! failed local write can require reconciliation; external calls are not retried.
use crate::client::{StripeBillingClient, StripeResource};
use chrono::{Duration, Utc};
use forklaunch_billing_base::services::{BaseBillingService, BillingStore};
use forklaunch_billing_interface::services::{
    BillingPortalDto, BillingPortalService, CheckoutSessionDto, CheckoutSessionService,
    CreateBillingPortalDto, CreateCheckoutSessionDto, CreatePaymentLinkDto, CreatePlanDto,
    CreateSubscriptionDto, PaymentLinkDto, PaymentLinkService, PlanDto, PlanService,
    SubscriptionDto, SubscriptionService, UpdateBillingPortalDto, UpdatePaymentLinkDto,
    UpdatePlanDto, UpdateSubscriptionDto,
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::{Map, Value, json};

pub struct StripeBillingService<S> {
    base: BaseBillingService<S>,
    client: StripeBillingClient,
}
impl<S: BillingStore> StripeBillingService<S> {
    #[must_use]
    pub const fn new(base: BaseBillingService<S>, client: StripeBillingClient) -> Self {
        Self { base, client }
    }
    #[must_use]
    pub const fn base(&self) -> &BaseBillingService<S> {
        &self.base
    }
    #[must_use]
    pub const fn client(&self) -> &StripeBillingClient {
        &self.client
    }
    async fn expanded_plan(&self, id: &str) -> BlueprintResult<Value> {
        self.client
            .retrieve(
                StripeResource::Plan,
                external_id(id)?,
                &json!({"expand":["product"]}),
            )
            .await
    }
    async fn read_plan(&self, id: &str) -> BlueprintResult<Value> {
        let provider = self.expanded_plan(id).await?;
        validate_read_identity(&provider, id, "plan")?;
        Ok(provider)
    }
    async fn enrich_subscription(
        &self,
        mut value: SubscriptionDto,
    ) -> BlueprintResult<SubscriptionDto> {
        let provider = self
            .client
            .retrieve(
                StripeResource::Subscription,
                external_id(&value.external_id)?,
                &json!({}),
            )
            .await?;
        validate_read_identity(&provider, &value.external_id, "subscription")?;
        value.provider_fields = Some(provider);
        Ok(value)
    }
    async fn link_status(&self, id: &str, status: &str) -> BlueprintResult<()> {
        self.client
            .update(
                StripeResource::PaymentLink,
                id,
                &json!({"metadata":{"status":status}}),
            )
            .await?;
        Ok(())
    }
    async fn checkout_status(&self, id: &str, status: &str) -> BlueprintResult<()> {
        self.client
            .update(
                StripeResource::CheckoutSession,
                id,
                &json!({"metadata":{"status":status}}),
            )
            .await?;
        Ok(())
    }
}
impl<S: BillingStore> PlanService for StripeBillingService<S> {
    async fn create_plan(&self, mut request: CreatePlanDto) -> BlueprintResult<PlanDto> {
        let fields = merge_fields(
            request.provider_fields.take(),
            json!({"amount":request.price,"interval":request.cadence,"product":request.name,"currency":request.currency}),
        )?;
        let created = self.client.create(StripeResource::Plan, &fields).await?;
        let plan = self.expanded_plan(provider_id(&created)?).await?;
        provider_id(&plan)?.clone_into(&mut request.external_id);
        request.billing_provider = "stripe".into();
        request.provider_fields = Some(product(&plan)?);
        self.base.create_plan(request).await
    }
    async fn get_plan(&self, id: &str) -> BlueprintResult<PlanDto> {
        let mut value = self.base.get_plan(id).await?;
        value.provider_fields = Some(product(&self.read_plan(&value.external_id).await?)?);
        Ok(value)
    }
    async fn update_plan(&self, mut request: UpdatePlanDto) -> BlueprintResult<PlanDto> {
        let original = self.base.get_plan(&request.id).await?;
        let existing = self
            .client
            .retrieve(
                StripeResource::Plan,
                external_id(&original.external_id)?,
                &json!({}),
            )
            .await?;
        validate_read_identity(&existing, &original.external_id, "plan")?;
        let existing_product = product(&existing)?;
        let product_id = existing_product
            .as_str()
            .or_else(|| existing_product["id"].as_str())
            .ok_or_else(invalid_response)?;
        let mut overrides = json!({"product":product_id,"interval":request.cadence.as_ref().map_or_else(||existing["interval"].clone(),|v|json!(v)),"currency":request.currency.as_ref().map_or_else(||existing["currency"].clone(),|v|json!(v))});
        if let Some(amount) = request
            .price
            .map(|v| json!(v))
            .or_else(|| existing.get("amount").filter(|v| !v.is_null()).cloned())
        {
            overrides["amount"] = amount;
        }
        let mut fields = merge_fields(request.provider_fields.take(), overrides.clone())?;
        if overrides.get("amount").is_none() {
            fields
                .as_object_mut()
                .ok_or_else(invalid_response)?
                .remove("amount");
        }
        self.client
            .delete(StripeResource::Plan, &original.external_id)
            .await?;
        let created = self.client.create(StripeResource::Plan, &fields).await?;
        let plan = self.expanded_plan(provider_id(&created)?).await?;
        request.external_id = Some(provider_id(&plan)?.to_owned());
        request.billing_provider = Some("stripe".into());
        request.provider_fields = Some(product(&plan)?);
        self.base.update_plan(request).await
    }
    async fn delete_plan(&self, id: &str) -> BlueprintResult<()> {
        let value = self.base.get_plan(id).await?;
        self.client
            .delete(StripeResource::Plan, external_id(&value.external_id)?)
            .await?;
        self.base.delete_plan(id).await
    }
    async fn list_plans(&self, ids: Option<&[String]>) -> BlueprintResult<Vec<PlanDto>> {
        let mut result = Vec::new();
        for mut value in self.base.list_plans(ids).await? {
            if let Ok(provider) = self
                .read_plan(&value.external_id)
                .await
                .and_then(|v| product(&v))
            {
                value.provider_fields = Some(provider);
                result.push(value);
            }
        }
        Ok(result)
    }
}
impl<S: BillingStore> SubscriptionService for StripeBillingService<S> {
    async fn create_subscription(
        &self,
        mut request: CreateSubscriptionDto,
    ) -> BlueprintResult<SubscriptionDto> {
        let fields = merge_fields(
            request.provider_fields.take(),
            json!({"customer":request.party_id,"items":[{"plan":request.product_id}]}),
        )?;
        let provider = self
            .client
            .create(StripeResource::Subscription, &fields)
            .await?;
        provider_id(&provider)?.clone_into(&mut request.external_id);
        request.billing_provider = "stripe".into();
        request.provider_fields = Some(provider);
        self.base.create_subscription(request).await
    }
    async fn get_subscription(&self, id: &str) -> BlueprintResult<SubscriptionDto> {
        self.enrich_subscription(self.base.get_subscription(id).await?)
            .await
    }
    async fn get_user_subscription(&self, id: &str) -> BlueprintResult<SubscriptionDto> {
        self.enrich_subscription(self.base.get_user_subscription(id).await?)
            .await
    }
    async fn get_organization_subscription(&self, id: &str) -> BlueprintResult<SubscriptionDto> {
        self.enrich_subscription(self.base.get_organization_subscription(id).await?)
            .await
    }
    async fn find_subscription_id_by_external_id(
        &self,
        id: &str,
    ) -> BlueprintResult<Option<String>> {
        self.base.find_subscription_id_by_external_id(id).await
    }
    async fn update_subscription(
        &self,
        mut request: UpdateSubscriptionDto,
    ) -> BlueprintResult<SubscriptionDto> {
        let original = self.base.get_subscription(&request.id).await?;
        let existing = self
            .client
            .retrieve(
                StripeResource::Subscription,
                external_id(&original.external_id)?,
                &json!({}),
            )
            .await?;
        validate_read_identity(&existing, &original.external_id, "subscription")?;
        let items = existing["items"]["data"]
            .as_array()
            .ok_or_else(invalid_response)?
            .iter()
            .map(|item| {
                let mut value = json!({"id":item["id"]});
                if let Some(plan) = request
                    .product_id
                    .as_ref()
                    .filter(|v| !v.is_empty())
                    .map(|v| json!(v))
                    .or_else(|| item.get("plan").and_then(|v| v.get("id")).cloned())
                {
                    value["plan"] = plan;
                }
                value
            })
            .collect::<Vec<_>>();
        let fields = merge_fields(request.provider_fields.take(), json!({"items":items}))?;
        let provider = self
            .client
            .update(StripeResource::Subscription, &original.external_id, &fields)
            .await?;
        request.external_id = Some(provider_id(&provider)?.into());
        request.billing_provider = Some("stripe".into());
        request.provider_fields = Some(provider);
        self.base.update_subscription(request).await
    }
    async fn delete_subscription(&self, id: &str) -> BlueprintResult<()> {
        let value = self.base.get_subscription(id).await?;
        self.client
            .delete(
                StripeResource::Subscription,
                external_id(&value.external_id)?,
            )
            .await?;
        self.base.delete_subscription(id).await
    }
    async fn list_subscriptions(
        &self,
        ids: Option<&[String]>,
    ) -> BlueprintResult<Vec<SubscriptionDto>> {
        let mut result = Vec::new();
        for value in self.base.list_subscriptions(ids).await? {
            if let Ok(value) = self.enrich_subscription(value).await {
                result.push(value);
            }
        }
        Ok(result)
    }
    async fn cancel_subscription(&self, id: &str) -> BlueprintResult<()> {
        let value = self.base.get_subscription(id).await?;
        self.client
            .delete(
                StripeResource::Subscription,
                external_id(&value.external_id)?,
            )
            .await?;
        self.base.cancel_subscription(id).await
    }
    async fn resume_subscription(&self, id: &str) -> BlueprintResult<()> {
        let value = self.base.get_subscription(id).await?;
        self.client
            .action(
                StripeResource::Subscription,
                external_id(&value.external_id)?,
                "resume",
                &json!({}),
            )
            .await?;
        self.base.resume_subscription(id).await
    }
}
impl<S: BillingStore> CheckoutSessionService for StripeBillingService<S> {
    async fn create_checkout_session(
        &self,
        mut request: CreateCheckoutSessionDto,
    ) -> BlueprintResult<CheckoutSessionDto> {
        let mut overrides = json!({"mode":"subscription","payment_method_types":request.payment_methods,"currency":request.currency});
        if let Some(value) = &request.success_redirect_uri {
            overrides["success_url"] = json!(value);
        }
        if let Some(value) = &request.cancel_redirect_uri {
            overrides["cancel_url"] = json!(value);
        }
        let mut fields = merge_fields(request.provider_fields.take(), overrides)?;
        for (key, missing) in [
            ("success_url", request.success_redirect_uri.is_none()),
            ("cancel_url", request.cancel_redirect_uri.is_none()),
        ] {
            if missing {
                fields
                    .as_object_mut()
                    .ok_or_else(invalid_response)?
                    .remove(key);
            }
        }
        let provider = self
            .client
            .create(StripeResource::CheckoutSession, &fields)
            .await?;
        request.id = Some(provider_id(&provider)?.into());
        request.uri = provider_uri(&provider)?;
        request.expires_at = Utc::now() + Duration::minutes(5);
        request.provider_fields = Some(provider);
        self.base.create_checkout_session(request).await
    }
    async fn get_checkout_session(&self, id: &str) -> BlueprintResult<CheckoutSessionDto> {
        let provider = self
            .client
            .retrieve(StripeResource::CheckoutSession, id, &json!({}))
            .await?;
        let mut value = self.base.get_checkout_session(id).await?;
        value.provider_fields = Some(provider);
        Ok(value)
    }
    async fn expire_checkout_session(&self, id: &str) -> BlueprintResult<()> {
        self.client
            .action(StripeResource::CheckoutSession, id, "expire", &json!({}))
            .await?;
        self.base.expire_checkout_session(id).await
    }
    async fn handle_checkout_success(&self, id: &str) -> BlueprintResult<()> {
        self.checkout_status(id, "SUCCESS").await?;
        self.base.handle_checkout_success(id).await
    }
    async fn handle_checkout_failure(&self, id: &str) -> BlueprintResult<()> {
        self.checkout_status(id, "FAILED").await?;
        self.base.handle_checkout_failure(id).await
    }
}
impl<S: BillingStore> PaymentLinkService for StripeBillingService<S> {
    async fn create_payment_link(
        &self,
        mut request: CreatePaymentLinkDto,
    ) -> BlueprintResult<PaymentLinkDto> {
        if !request
            .provider_fields
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|fields| fields.get("line_items"))
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().all(Value::is_object))
        {
            return Err(BlueprintError::new(
                "billing_invalid_payment_link_options",
                "Payment link provider options require a line_items array of objects",
            ));
        }
        let fields = merge_fields(
            request.provider_fields.take(),
            json!({"payment_method_types":request.payment_methods,"currency":request.currency}),
        )?;
        let provider = self
            .client
            .create(StripeResource::PaymentLink, &fields)
            .await?;
        request.id = Some(provider_id(&provider)?.into());
        request.provider_fields = Some(provider);
        self.base.create_payment_link(request).await
    }
    async fn update_payment_link(
        &self,
        mut request: UpdatePaymentLinkDto,
    ) -> BlueprintResult<PaymentLinkDto> {
        let mut overrides = json!({});
        if let Some(value) = &request.payment_methods {
            overrides["payment_method_types"] = json!(value);
        }
        let mut fields = merge_fields(request.provider_fields.take(), overrides)?;
        if request.payment_methods.is_none() {
            fields
                .as_object_mut()
                .ok_or_else(invalid_response)?
                .remove("payment_method_types");
        }
        let provider = self
            .client
            .update(StripeResource::PaymentLink, &request.id, &fields)
            .await?;
        request.id = provider_id(&provider)?.into();
        request.amount = Some(line_amount(&provider)?);
        request.provider_fields = Some(provider);
        self.base.update_payment_link(request).await
    }
    async fn get_payment_link(&self, id: &str) -> BlueprintResult<PaymentLinkDto> {
        let provider = self
            .client
            .retrieve(StripeResource::PaymentLink, id, &json!({}))
            .await?;
        let mut value = self.base.get_payment_link(id).await?;
        value.provider_fields = Some(provider);
        Ok(value)
    }
    async fn expire_payment_link(&self, id: &str) -> BlueprintResult<()> {
        self.link_status(id, "EXPIRED").await?;
        self.base.expire_payment_link(id).await
    }
    async fn handle_payment_success(&self, id: &str) -> BlueprintResult<()> {
        self.link_status(id, "COMPLETED").await?;
        self.base.handle_payment_success(id).await
    }
    async fn handle_payment_failure(&self, id: &str) -> BlueprintResult<()> {
        self.link_status(id, "FAILED").await?;
        self.base.handle_payment_failure(id).await
    }
    async fn list_payment_links(
        &self,
        ids: Option<&[String]>,
    ) -> BlueprintResult<Vec<PaymentLinkDto>> {
        let provider = self
            .client
            .list(StripeResource::PaymentLink, &json!({"active":true}))
            .await?;
        let data = provider["data"].as_array().ok_or_else(invalid_response)?;
        let mut result = Vec::new();
        for mut value in self.base.list_payment_links(ids).await? {
            if let Some(provider) = data.iter().find(|v| v["id"].as_str() == Some(&value.id)) {
                value.provider_fields = Some(provider.clone());
                result.push(value);
            }
        }
        Ok(result)
    }
}
impl<S: BillingStore> BillingPortalService for StripeBillingService<S> {
    async fn create_billing_portal_session(
        &self,
        mut request: CreateBillingPortalDto,
    ) -> BlueprintResult<BillingPortalDto> {
        let fields = merge_fields(
            request.provider_fields.take(),
            json!({"customer":request.customer_id}),
        )?;
        let provider = self
            .client
            .create(StripeResource::BillingPortal, &fields)
            .await?;
        request.id = Some(provider_id(&provider)?.into());
        request.uri = provider_uri(&provider)?;
        request.expires_at = Utc::now() + Duration::minutes(5);
        request.provider_fields = Some(provider);
        self.base.create_billing_portal_session(request).await
    }
    async fn get_billing_portal_session(&self, id: &str) -> BlueprintResult<BillingPortalDto> {
        self.base.get_billing_portal_session(id).await
    }
    async fn update_billing_portal_session(
        &self,
        mut request: UpdateBillingPortalDto,
    ) -> BlueprintResult<BillingPortalDto> {
        let original = self.base.get_billing_portal_session(&request.id).await?;
        let customer = request
            .customer_id
            .as_ref()
            .filter(|v| !v.is_empty())
            .unwrap_or(&original.customer_id);
        let fields = merge_fields(request.provider_fields.take(), json!({"customer":customer}))?;
        let provider = self
            .client
            .create(StripeResource::BillingPortal, &fields)
            .await?;
        request.uri = provider_uri(&provider)?;
        request.expires_at = Some(Utc::now() + Duration::minutes(5));
        request.provider_fields = Some(provider);
        self.base.update_billing_portal_session(request).await
    }
    async fn expire_billing_portal_session(&self, id: &str) -> BlueprintResult<()> {
        self.base.expire_billing_portal_session(id).await
    }
}
fn merge_fields(fields: Option<Value>, overrides: Value) -> BlueprintResult<Value> {
    let mut fields = match fields {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(fields)) => fields,
        _ => {
            return Err(BlueprintError::new(
                "billing_validation",
                "Invalid Stripe billing fields",
            ));
        }
    };
    let Value::Object(overrides) = overrides else {
        return Err(invalid_response());
    };
    fields.extend(overrides);
    Ok(Value::Object(fields))
}
fn provider_id(provider: &Value) -> BlueprintResult<&str> {
    provider["id"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(invalid_response)
}
fn product(provider: &Value) -> BlueprintResult<Value> {
    provider
        .get("product")
        .filter(|v| v.is_object() || v.is_string())
        .cloned()
        .ok_or_else(invalid_response)
}
fn provider_uri(provider: &Value) -> BlueprintResult<Option<String>> {
    match provider.get("url") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(invalid_response()),
    }
}
fn external_id(id: &str) -> BlueprintResult<&str> {
    if id.is_empty() {
        Err(BlueprintError::new(
            "billing_not_found",
            "Billing provider reference not found",
        ))
    } else {
        Ok(id)
    }
}
fn line_amount(provider: &Value) -> BlueprintResult<f64> {
    let Some(data) = provider.get("line_items").and_then(|v| v.get("data")) else {
        return Ok(0.0);
    };
    let total = data
        .as_array()
        .ok_or_else(invalid_response)?
        .iter()
        .try_fold(0.0, |total, item| {
            item["amount_total"]
                .as_f64()
                .map(|amount| total + amount)
                .ok_or_else(invalid_response)
        })?;
    if total.is_finite() {
        Ok(total)
    } else {
        Err(invalid_response())
    }
}
fn invalid_response() -> BlueprintError {
    BlueprintError::new("stripe_invalid_response", "Invalid Stripe billing response")
}

fn validate_read_identity(provider: &Value, expected: &str, kind: &str) -> BlueprintResult<()> {
    if provider["id"].as_str() != Some(expected) || provider["object"].as_str() != Some(kind) {
        return Err(invalid_response());
    }
    Ok(())
}
