#![forbid(unsafe_code)]

use std::cell::RefCell;

use forklaunch_billing_interface::{
    BillingService, CheckoutSession, Customer, Entitlement, Invoice, Subscription, UsageRecord,
};
use forklaunch_blueprint_core::{AuditContext, BlueprintResult, Page, PageRequest};

#[derive(Debug, Default)]
pub struct InMemoryBillingService {
    customers: Vec<Customer>,
    subscriptions: Vec<Subscription>,
    invoices: Vec<Invoice>,
    entitlements: Vec<Entitlement>,
    usage: RefCell<Vec<UsageRecord>>,
}

impl InMemoryBillingService {
    #[must_use]
    pub fn new(customers: Vec<Customer>) -> Self {
        Self {
            customers,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_subscription(mut self, subscription: Subscription) -> Self {
        self.subscriptions.push(subscription);
        self
    }

    #[must_use]
    pub fn with_invoice(mut self, invoice: Invoice) -> Self {
        self.invoices.push(invoice);
        self
    }

    #[must_use]
    pub fn with_entitlement(mut self, entitlement: Entitlement) -> Self {
        self.entitlements.push(entitlement);
        self
    }

    /// Returns the total recorded usage quantity for a customer's feature.
    #[must_use]
    pub fn recorded_usage(&self, customer_id: &str, feature: &str) -> u64 {
        self.usage
            .borrow()
            .iter()
            .filter(|record| record.customer_id == customer_id && record.feature == feature)
            .map(|record| record.quantity)
            .sum()
    }
}

impl BillingService for InMemoryBillingService {
    fn get_customer(&self, id: &str) -> BlueprintResult<Option<Customer>> {
        Ok(self
            .customers
            .iter()
            .find(|customer| customer.id == id)
            .cloned())
    }

    fn list_customers(&self, request: PageRequest) -> BlueprintResult<Page<Customer>> {
        let start = request
            .cursor
            .as_ref()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .unwrap_or(0);
        let end = start
            .saturating_add(request.limit)
            .min(self.customers.len());
        let mut page = Page::new(self.customers[start..end].to_vec());

        if end < self.customers.len() {
            page = page.with_next_cursor(end.to_string());
        }

        Ok(page)
    }

    fn list_subscriptions(&self, customer_id: &str) -> BlueprintResult<Vec<Subscription>> {
        Ok(self
            .subscriptions
            .iter()
            .filter(|subscription| subscription.customer_id == customer_id)
            .cloned()
            .collect())
    }

    fn create_checkout_session(
        &self,
        customer_id: &str,
        plan_id: &str,
        context: &AuditContext,
    ) -> BlueprintResult<CheckoutSession> {
        Ok(CheckoutSession {
            id: format!("checkout_{}_{}", customer_id, context.correlation_id),
            customer_id: customer_id.to_owned(),
            url: format!("https://billing.local/checkout/{customer_id}/{plan_id}"),
        })
    }

    fn list_invoices(&self, customer_id: &str) -> BlueprintResult<Vec<Invoice>> {
        Ok(self
            .invoices
            .iter()
            .filter(|invoice| invoice.customer_id == customer_id)
            .cloned()
            .collect())
    }

    fn has_entitlement(&self, customer_id: &str, feature: &str) -> BlueprintResult<bool> {
        let Some(entitlement) = self.entitlements.iter().find(|entitlement| {
            entitlement.customer_id == customer_id && entitlement.feature == feature
        }) else {
            return Ok(false);
        };

        Ok(match entitlement.limit {
            Some(limit) => self.recorded_usage(customer_id, feature) < limit,
            None => true,
        })
    }

    fn record_usage(&self, record: UsageRecord) -> BlueprintResult<()> {
        self.usage.borrow_mut().push(record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use forklaunch_billing_interface::SubscriptionStatus;

    use super::*;

    #[test]
    fn in_memory_billing_lists_customers_and_subscriptions() {
        let service =
            InMemoryBillingService::new(vec![Customer::new("customer_1", "test@example.com")])
                .with_subscription(Subscription {
                    id: "sub_1".to_owned(),
                    customer_id: "customer_1".to_owned(),
                    plan_id: "pro".to_owned(),
                    status: SubscriptionStatus::Active,
                });

        let page = service
            .list_customers(PageRequest::first_page(10))
            .expect("customers should list");
        let checkout = service
            .create_checkout_session("customer_1", "pro", &AuditContext::system("corr_1"))
            .expect("checkout should be created");

        assert_eq!(page.items.len(), 1);
        assert_eq!(
            service
                .list_subscriptions("customer_1")
                .expect("subscriptions should list")
                .len(),
            1
        );
        assert!(checkout.url.contains("customer_1/pro"));
    }

    #[test]
    fn in_memory_billing_enforces_metered_entitlements() {
        use forklaunch_billing_interface::{Entitlement, UsageRecord};

        let service =
            InMemoryBillingService::new(vec![Customer::new("customer_1", "test@example.com")])
                .with_entitlement(Entitlement {
                    customer_id: "customer_1".to_owned(),
                    feature: "api_calls".to_owned(),
                    limit: Some(2),
                });

        assert!(
            service
                .has_entitlement("customer_1", "api_calls")
                .expect("entitlement check should succeed")
        );

        service
            .record_usage(UsageRecord {
                customer_id: "customer_1".to_owned(),
                feature: "api_calls".to_owned(),
                quantity: 2,
                recorded_at_unix_seconds: 1000,
            })
            .expect("usage should record");

        // limit reached -> no longer entitled
        assert!(
            !service
                .has_entitlement("customer_1", "api_calls")
                .expect("entitlement check should succeed")
        );
        assert_eq!(service.recorded_usage("customer_1", "api_calls"), 2);
    }
}
