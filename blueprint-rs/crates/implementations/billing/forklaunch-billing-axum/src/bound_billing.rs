//! One explicit finite selector for compatible domain-specific authority profiles.
use super::{
    BillingBackend, BoundCheckoutConfig, BoundCheckoutV1, BoundPortalV1, bound_checkout_contracts,
    bound_plan_read_contracts, bound_portal_contracts, bound_subscription_read_contracts,
};
use forklaunch_billing_base::services::postgres::{
    StripeBindingScope, TrustedCheckoutPublisher, TrustedPortalPublisher,
};
use forklaunch_billing_stripe::bound_creation::BoundPaymentLinkCreationProfile;
use forklaunch_billing_stripe::bound_plan_creation::BoundPlanCreationProfile;
use forklaunch_billing_stripe::bound_portal_creation::BoundPortalCreationProfile;
use forklaunch_billing_stripe::bound_subscription_creation::{
    BoundSubscriptionCreationProfile, SubscriptionCollectionPolicy,
};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult, contract::RouteContract};
use serde_json::Value;
use uuid::Uuid;

pub(super) fn configuration() -> BlueprintError {
    BlueprintError::new(
        "billing_configuration",
        "Invalid bound billing configuration",
    )
}
/// Server-owned finite Portal policy. No credentials or raw provider options.
pub struct BoundPortalConfig {
    pub(super) profile: BoundPortalCreationProfile,
}
/// Shared scope, optional explicitly selected domains; no Debug/Serde or credentials.
pub(super) struct PaymentLinkReadConfig {
    pub(super) profile: BoundPaymentLinkCreationProfile,
    pub(super) max_records: usize,
}
pub struct BoundBillingConfig {
    checkout: Option<BoundCheckoutConfig>,
    portal: Option<BoundPortalConfig>,
    plan_read: Option<BoundPlanCreationProfile>,
    subscription_read: Option<BoundSubscriptionCreationProfile>,
    payment_link_read: Option<PaymentLinkReadConfig>,
}
fn exact(value: &Value, required: &[&str], optional: &[&str]) -> BlueprintResult<()> {
    let object = value.as_object().ok_or_else(configuration)?;
    if required.iter().any(|key| !object.contains_key(*key))
        || object
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(configuration());
    }
    Ok(())
}
fn subscription_profile(
    scope: &StripeBindingScope,
    part: &Value,
) -> BlueprintResult<BoundSubscriptionCreationProfile> {
    exact(part, &["plans", "collectionPolicy"], &[])?;
    if part["collectionPolicy"] != "automatic_default_incomplete_v1" {
        return Err(configuration());
    }
    let rows = part["plans"].as_array().ok_or_else(configuration)?;
    if rows.is_empty() || rows.len() > 1000 {
        return Err(configuration());
    }
    let mut plans = Vec::with_capacity(rows.len());
    for row in rows {
        exact(row, &["localPlanId", "creationOperationId"], &[])?;
        let id = |key: &str| {
            Uuid::parse_str(row[key].as_str().ok_or_else(configuration)?)
                .map_err(|_| configuration())
        };
        plans.push((id("localPlanId")?, id("creationOperationId")?));
    }
    BoundSubscriptionCreationProfile::trusted(
        scope.clone(),
        plans,
        SubscriptionCollectionPolicy::AutomaticDefaultIncomplete,
    )
    .map_err(|_| configuration())
}
fn payment_link_profile(
    scope: &StripeBindingScope,
    part: &Value,
) -> BlueprintResult<PaymentLinkReadConfig> {
    exact(part, &["trustedPrices"], &["maxRecords"])?;
    let prices: Vec<String> =
        serde_json::from_value(part["trustedPrices"].clone()).map_err(|_| configuration())?;
    let max_records = match part.get("maxRecords") {
        Some(value) => value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| (1..=1000).contains(v))
            .ok_or_else(configuration)?,
        None => 100,
    };
    Ok(PaymentLinkReadConfig {
        profile: BoundPaymentLinkCreationProfile::trusted(scope.clone(), prices)
            .map_err(|_| configuration())?,
        max_records,
    })
}
impl BoundBillingConfig {
    /// Parses at most 64 KiB of exact configuration. At least one domain is required.
    /// # Errors
    /// Rejects unknown/null/empty domains, malformed scope, redirects or provenance.
    pub fn from_json(text: &str) -> BlueprintResult<Self> {
        if text.len() > 65_536 {
            return Err(configuration());
        }
        let value: Value = serde_json::from_str(text).map_err(|_| configuration())?;
        exact(
            &value,
            &["deploymentLabel", "accountId", "livemode"],
            &[
                "checkout",
                "portal",
                "planRead",
                "subscriptionRead",
                "paymentLinkRead",
            ],
        )?;
        let scope = StripeBindingScope::administrative(
            value["deploymentLabel"]
                .as_str()
                .ok_or_else(configuration)?
                .to_owned(),
            value["accountId"]
                .as_str()
                .ok_or_else(configuration)?
                .to_owned(),
            value["livemode"].as_bool().ok_or_else(configuration)?,
        )
        .map_err(|_| configuration())?;
        let checkout = if let Some(part) = value.get("checkout") {
            exact(part, &["plans", "successUrl", "cancelUrl"], &[])?;
            let mut legacy = part.clone();
            for key in ["deploymentLabel", "accountId", "livemode"] {
                legacy[key] = value[key].clone();
            }
            Some(BoundCheckoutConfig::from_json(&legacy.to_string()).map_err(|_| configuration())?)
        } else {
            None
        };
        let portal = if let Some(part) = value.get("portal") {
            exact(part, &["configurationId", "returnUrl"], &[])?;
            Some(BoundPortalConfig {
                profile: BoundPortalCreationProfile::trusted(
                    scope.clone(),
                    part["configurationId"]
                        .as_str()
                        .ok_or_else(configuration)?
                        .to_owned(),
                    part["returnUrl"]
                        .as_str()
                        .ok_or_else(configuration)?
                        .to_owned(),
                )
                .map_err(|_| configuration())?,
            })
        } else {
            None
        };
        let plan_read = if let Some(part) = value.get("planRead") {
            exact(part, &["trustedProducts"], &[])?;
            let products: Vec<String> = serde_json::from_value(part["trustedProducts"].clone())
                .map_err(|_| configuration())?;
            Some(
                BoundPlanCreationProfile::trusted(scope.clone(), products)
                    .map_err(|_| configuration())?,
            )
        } else {
            None
        };
        let subscription_read = value
            .get("subscriptionRead")
            .map(|part| subscription_profile(&scope, part))
            .transpose()?;
        let payment_link_read = value
            .get("paymentLinkRead")
            .map(|part| payment_link_profile(&scope, part))
            .transpose()?;
        if checkout.is_none()
            && portal.is_none()
            && plan_read.is_none()
            && subscription_read.is_none()
            && payment_link_read.is_none()
        {
            return Err(configuration());
        }
        Ok(Self {
            checkout,
            portal,
            plan_read,
            subscription_read,
            payment_link_read,
        })
    }
    #[must_use]
    pub const fn has_checkout(&self) -> bool {
        self.checkout.is_some()
    }
    #[must_use]
    pub const fn has_portal(&self) -> bool {
        self.portal.is_some()
    }
    #[must_use]
    pub const fn has_plan_read(&self) -> bool {
        self.plan_read.is_some()
    }
    #[must_use]
    pub const fn has_subscription_read(&self) -> bool {
        self.subscription_read.is_some()
    }
    #[must_use]
    pub const fn has_payment_link_read(&self) -> bool {
        self.payment_link_read.is_some()
    }
    /// Only creation families need a separately trusted publication connection.
    #[must_use]
    pub const fn requires_publisher(&self) -> bool {
        self.has_checkout() || self.has_portal()
    }
    /// Shared runtime/artifact contracts, with no server configuration embedded.
    #[must_use]
    pub fn contracts(&self) -> Vec<RouteContract> {
        let mut routes = Vec::new();
        if self.has_checkout() {
            routes.extend(bound_checkout_contracts());
        }
        if self.has_portal() {
            routes.extend(bound_portal_contracts());
        }
        if self.has_plan_read() {
            routes.extend(bound_plan_read_contracts());
        }
        if self.has_subscription_read() {
            routes.extend(bound_subscription_read_contracts());
        }
        if let Some(profile) = &self.payment_link_read {
            routes.extend(super::bound_payment_link_read::contracts(
                profile.max_records,
            ));
        }
        routes
    }
}
/// Explicit independent capabilities. Creation needs publishers; Plan read does not.
pub struct BoundBillingV1 {
    checkout: Option<BoundCheckoutV1>,
    portal: Option<BoundPortalV1>,
    plan_read: Option<BoundPlanCreationProfile>,
    subscription_read: Option<BoundSubscriptionCreationProfile>,
    payment_link_read: Option<PaymentLinkReadConfig>,
}
impl BoundBillingV1 {
    /// # Errors
    /// Requires exactly one matching publisher per selected creation domain; no fallback.
    pub fn trusted(
        configuration: BoundBillingConfig,
        checkout: Option<TrustedCheckoutPublisher>,
        portal: Option<TrustedPortalPublisher>,
    ) -> BlueprintResult<Self> {
        if configuration.has_checkout() != checkout.is_some()
            || configuration.has_portal() != portal.is_some()
        {
            return Err(self::configuration());
        }
        Ok(Self {
            plan_read: configuration.plan_read,
            subscription_read: configuration.subscription_read,
            payment_link_read: configuration.payment_link_read,
            checkout: configuration
                .checkout
                .zip(checkout)
                .map(|(c, p)| BoundCheckoutV1::trusted(c, p)),
            portal: configuration
                .portal
                .zip(portal)
                .map(|(c, p)| BoundPortalV1::trusted(c, p)),
        })
    }
}
impl BillingBackend {
    /// Select once after backup configuration; all selected authorities validate before listening.
    /// # Errors
    /// Rejects mixed builders, base/managed/backup and any invalid publisher authority.
    pub async fn with_bound_billing(mut self, profile: BoundBillingV1) -> BlueprintResult<Self> {
        if self.bound_checkout.is_some()
            || self.bound_portal.is_some()
            || self.bound_plan_read.is_some()
            || self.bound_subscription_read.is_some()
            || self.bound_payment_link_read.is_some()
        {
            return Err(configuration());
        }
        if let Some(checkout) = profile.checkout {
            self = self.with_bound_checkout(checkout).await?;
        }
        if let Some(portal) = profile.portal {
            self = self.with_bound_portal(portal).await?;
        }
        if let Some(plan_read) = profile.plan_read {
            self = self.with_bound_plan_read(plan_read).await?;
        }
        if let Some(subscription_read) = profile.subscription_read {
            self = self.with_bound_subscription_read(subscription_read).await?;
        }
        if let Some(payment_link_read) = profile.payment_link_read {
            self = self.with_bound_payment_link_read(payment_link_read).await?;
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn valid() -> Value {
        json!({"deploymentLabel":"owned","accountId":"acct_owned","livemode":false,"portal":{"configurationId":"bpc_owned","returnUrl":"https://example.test/return"}})
    }
    #[test]
    fn payment_link_read_selects_no_creation_authority() {
        let mut value = json!({"deploymentLabel":"owned","accountId":"acct_owned","livemode":false,"paymentLinkRead":{"trustedPrices":["price_owned"]}});
        let config = BoundBillingConfig::from_json(&value.to_string()).unwrap();
        assert!(config.has_payment_link_read());
        assert!(!config.requires_publisher());
        assert_eq!(
            config.contracts(),
            super::super::bound_payment_link_read_contracts()
        );
        assert!(BoundBillingV1::trusted(config, None, None).is_ok());
        for bad in [
            Value::Null,
            json!({}),
            json!({"trustedPrices":[]}),
            json!({"trustedPrices":["price_owned"],"maxRecords":0}),
            json!({"trustedPrices":["price_owned"],"maxRecords":1001}),
            json!({"trustedPrices":["price_owned"],"maxRecords":null}),
            json!({"trustedPrices":["not-a-price"]}),
            json!({"trustedPrices":["price_owned"],"create":true}),
        ] {
            value["paymentLinkRead"] = bad;
            assert!(BoundBillingConfig::from_json(&value.to_string()).is_err());
        }
    }
    #[test]
    fn subscription_read_is_an_explicit_nonpublishing_capability() {
        let mut value = json!({"deploymentLabel":"owned","accountId":"acct_owned","livemode":false,"subscriptionRead":{"plans":[{"localPlanId":Uuid::new_v4(),"creationOperationId":Uuid::new_v4()}],"collectionPolicy":"automatic_default_incomplete_v1"}});
        let config = BoundBillingConfig::from_json(&value.to_string()).unwrap();
        assert!(config.has_subscription_read());
        assert!(!config.requires_publisher());
        assert_eq!(config.contracts(), bound_subscription_read_contracts());
        assert!(BoundBillingV1::trusted(config, None, None).is_ok());
        let valid = value["subscriptionRead"].clone();
        for invalid in [
            Value::Null,
            json!({}),
            json!({"plans":[],"collectionPolicy":"automatic_default_incomplete_v1"}),
            json!({"plans":valid["plans"],"collectionPolicy":"send_invoice"}),
            json!({"plans":valid["plans"],"collectionPolicy":"automatic_default_incomplete_v1","create":true}),
        ] {
            value["subscriptionRead"] = invalid;
            assert!(BoundBillingConfig::from_json(&value.to_string()).is_err());
        }
        value["subscriptionRead"] = valid.clone();
        value["subscriptionRead"]["plans"] = json!([valid["plans"][0], valid["plans"][0]]);
        assert!(BoundBillingConfig::from_json(&value.to_string()).is_err());
        value["subscriptionRead"] = valid;
        value["subscriptionRead"]["plans"][0]["localPlanId"] = json!(Uuid::nil());
        assert!(BoundBillingConfig::from_json(&value.to_string()).is_err());
    }
    #[test]
    fn aggregate_selects_only_explicit_domains_and_preserves_legacy_contracts() {
        let portal = BoundBillingConfig::from_json(&valid().to_string()).unwrap();
        assert!(!portal.has_checkout());
        assert!(portal.has_portal());
        assert_eq!(portal.contracts(), bound_portal_contracts());
        let mut both = valid();
        both["checkout"] = json!({"plans":[{"localPlanId":uuid::Uuid::new_v4(),"creationOperationId":uuid::Uuid::new_v4()}],"successUrl":"https://example.test/success","cancelUrl":"https://example.test/cancel"});
        let parsed = BoundBillingConfig::from_json(&both.to_string()).unwrap();
        assert_eq!(
            parsed.contracts(),
            [bound_checkout_contracts(), bound_portal_contracts()].concat()
        );
        both.as_object_mut().unwrap().remove("portal");
        assert_eq!(
            BoundBillingConfig::from_json(&both.to_string())
                .unwrap()
                .contracts(),
            bound_checkout_contracts()
        );
        assert!(BoundBillingV1::trusted(portal, None, None).is_err());
    }
    #[test]
    fn plan_read_selects_no_publisher_or_create_capability() {
        let value = json!({"deploymentLabel":"owned","accountId":"acct_owned","livemode":false,"planRead":{"trustedProducts":["custom-product"]}});
        let config = BoundBillingConfig::from_json(&value.to_string()).unwrap();
        assert!(config.has_plan_read());
        assert!(!config.requires_publisher());
        assert_eq!(config.contracts(), bound_plan_read_contracts());
        assert!(BoundBillingV1::trusted(config, None, None).is_ok());
        for invalid in [
            json!(null),
            json!({}),
            json!({"trustedProducts":[]}),
            json!({"trustedProducts":[1]}),
            json!({"trustedProducts":["../product"]}),
            json!({"trustedProducts":["custom-product"],"create":true}),
        ] {
            let mut bad = value.clone();
            bad["planRead"] = invalid;
            assert!(BoundBillingConfig::from_json(&bad.to_string()).is_err());
        }
    }
    #[test]
    fn aggregate_rejects_partial_unknown_and_untrusted_options() {
        let mut missing = valid();
        missing.as_object_mut().unwrap().remove("portal");
        let mut unknown = valid();
        unknown["subscription"] = json!({});
        let mut null = valid();
        null["portal"] = Value::Null;
        let mut empty = valid();
        empty["portal"] = json!({});
        let mut raw = valid();
        raw["portal"]["stripeFields"] = json!({});
        let mut bad_url = valid();
        bad_url["portal"]["returnUrl"] = json!("https://secret@private.example");
        for value in [missing, unknown, null, empty, raw, bad_url] {
            let error = BoundBillingConfig::from_json(&value.to_string())
                .err()
                .unwrap();
            assert_eq!(error.message, "Invalid bound billing configuration");
        }
        assert!(BoundBillingConfig::from_json(&" ".repeat(65_537)).is_err());
    }
}
