//! One explicit finite selector for compatible domain-specific authority profiles.
use super::{
    BillingBackend, BoundCheckoutConfig, BoundCheckoutV1, BoundPortalV1, bound_checkout_contracts,
    bound_portal_contracts,
};
use forklaunch_billing_base::services::postgres::{
    StripeBindingScope, TrustedCheckoutPublisher, TrustedPortalPublisher,
};
use forklaunch_billing_stripe::bound_portal_creation::BoundPortalCreationProfile;
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult, contract::RouteContract};
use serde_json::Value;

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
pub struct BoundBillingConfig {
    checkout: Option<BoundCheckoutConfig>,
    portal: Option<BoundPortalConfig>,
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
            &["checkout", "portal"],
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
                    scope,
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
        if checkout.is_none() && portal.is_none() {
            return Err(configuration());
        }
        Ok(Self { checkout, portal })
    }
    #[must_use]
    pub const fn has_checkout(&self) -> bool {
        self.checkout.is_some()
    }
    #[must_use]
    pub const fn has_portal(&self) -> bool {
        self.portal.is_some()
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
        routes
    }
}
/// Explicit independent publisher capabilities; one pool may hold both grants.
pub struct BoundBillingV1 {
    checkout: Option<BoundCheckoutV1>,
    portal: Option<BoundPortalV1>,
}
impl BoundBillingV1 {
    /// # Errors
    /// Requires exactly one matching publisher per selected domain; no fallback.
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
        if self.bound_checkout.is_some() || self.bound_portal.is_some() {
            return Err(configuration());
        }
        if let Some(checkout) = profile.checkout {
            self = self.with_bound_checkout(checkout).await?;
        }
        if let Some(portal) = profile.portal {
            self = self.with_bound_portal(portal).await?;
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
