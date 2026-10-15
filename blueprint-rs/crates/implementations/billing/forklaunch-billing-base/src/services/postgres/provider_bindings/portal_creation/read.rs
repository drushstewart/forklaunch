//! Historical local Portal reads, never fresh provider evidence or URL renewal.
use super::super::DurableStripeBinding;
use super::{IntentRow, PostgresBillingStore, conflict, db_error, storage_error};
use crate::services::{BillingKind, Scope};
use chrono::{DateTime, Duration, Utc};
use forklaunch_blueprint_core::{BlueprintError, BlueprintResult};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

fn expired() -> BlueprintError {
    BlueprintError::new(
        "billing_portal_local_session_expired",
        "Stored portal session is outside its original local lifetime",
    )
}
fn lifetime(record: &Value, started: DateTime<Utc>, now: DateTime<Utc>) -> BlueprintResult<()> {
    let date = |key: &str| -> BlueprintResult<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(record[key].as_str().ok_or_else(storage_error)?)
            .map(|v| v.with_timezone(&Utc))
            .map_err(|_| storage_error())
    };
    let created = date("createdAt")?;
    let expires = date("expiresAt")?;
    if expires.signed_duration_since(created) != Duration::minutes(5) {
        return Err(conflict());
    }
    if now < started || started < created || now < created || now >= expires {
        return Err(expired());
    }
    Ok(())
}
impl PostgresBillingStore {
    /// Reads an actor-owned published Portal DTO without contacting a provider.
    /// Current record/alias/Customer proof must match the sealed creation receipt.
    /// Final matching SELECT is the local observation point; later changes are outside
    /// this read. Final clock after audit COMMIT must remain inside the original TTL.
    /// # Errors
    /// Denies absent/changed provenance, scope/config mismatch, expiry or audit failure.
    #[allow(clippy::too_many_lines)]
    pub async fn read_stored_portal_creation(
        &self,
        binding: &DurableStripeBinding,
        api_version: &str,
        configuration_id: &str,
        return_url: &str,
    ) -> BlueprintResult<(Value, DateTime<Utc>)> {
        let started = Utc::now();
        if binding.tenant != self.tenant.id() || binding.kind != BillingKind::BillingPortal {
            return Err(conflict());
        }
        let actor = self.binding_actor()?;
        let mut tx = self.transaction().await?;
        self.portal_runtime_role(&mut tx).await?;
        let operation:Option<String>=sqlx::query_scalar("SELECT operation_id::text FROM fl_billing.portal_creation_intents WHERE tenant_id=$1 AND public_id=$2::uuid")
            .bind(self.tenant.id()).bind(binding.public_id.to_string()).fetch_optional(&mut *tx).await.map_err(db_error)?;
        let operation =
            Uuid::parse_str(&operation.ok_or_else(conflict)?).map_err(|_| storage_error())?;
        let row = self
            .portal_intent_row(&mut tx, operation)
            .await?
            .ok_or_else(conflict)?;
        let intent = self.open_portal_intent(operation, &row)?;
        let receipt = self.open_portal_receipt(operation, &row)?;
        let fields = &intent["payload"]["fields"];
        if intent["tenant"] != self.tenant.id()
            || intent["actor"] != actor
            || intent["deployment"] != binding.scope.deployment_label
            || intent["account"] != binding.scope.account_id
            || intent["livemode"] != binding.scope.livemode
            || intent["apiVersion"] != api_version
            || intent["payload"]["customer"] != intent["customer"]
            || intent["payload"]["profile"] != "direct_portal_v1"
            || fields["configuration"] != configuration_id
            || fields["return_url"] != return_url
            || receipt["id"] != binding.storage_id
            || receipt["customerId"] != intent["customer"]
            || receipt["providerFields"]["id"] != binding.provider_id
            || receipt["providerFields"]["object"] != "billing_portal.session"
            || receipt["providerFields"]["livemode"] != binding.scope.livemode
            || receipt["providerFields"]["configuration"] != configuration_id
            || receipt["providerFields"]["return_url"] != return_url
            || receipt["providerFields"]["customer"] != fields["customer"]
            || receipt["uri"] != receipt["providerFields"]["url"]
            || receipt["uri"].as_str().is_none_or(|value| {
                value.len() > 65_536
                    || value
                        .trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
                        .is_empty()
            })
        {
            return Err(conflict());
        }
        let customer = intent["customer"].as_str().ok_or_else(storage_error)?;
        let observed = self
            .portal_read_observation(&mut tx, binding, api_version, operation, &row, customer)
            .await?;
        for key in ["customerData", "customerVersion", "customerBinding"] {
            if observed[key] != intent["dependencies"][key] {
                return Err(conflict());
            }
        }
        if observed["customerBinding"]["provider_customer_id"] != fields["customer"] {
            return Err(conflict());
        }
        lifetime(&receipt, started, Utc::now())?;
        let record = self
            .decode(
                &mut tx,
                Scope::Durable,
                BillingKind::BillingPortal,
                &binding.storage_id,
                observed["record"].clone(),
                "portal_stored_read",
            )
            .await?;
        if record != receipt {
            return Err(conflict());
        }
        // Auditing may wait. Observe every dependency again only after those awaits.
        let current = self
            .portal_read_observation(&mut tx, binding, api_version, operation, &row, customer)
            .await?;
        if current != observed {
            return Err(conflict());
        }
        tx.commit().await.map_err(db_error)?;
        let observed_at = Utc::now();
        lifetime(&record, started, observed_at)?;
        Ok((record, observed_at))
    }
    #[allow(clippy::too_many_arguments)]
    async fn portal_read_observation(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        binding: &DurableStripeBinding,
        api_version: &str,
        operation: Uuid,
        row: &IntentRow,
        customer: &str,
    ) -> BlueprintResult<Value> {
        let value:Option<sqlx::types::Json<Value>>=sqlx::query_scalar("SELECT jsonb_build_object('record',r.data,'recordVersion',r.xmin::text,'binding',to_jsonb(b),'bindingVersion',b.xmin::text,'customerBindingVersion',cb.xmin::text,'customerData',c.data,'customerVersion',c.xmin::text,'customerBinding',to_jsonb(cb)) FROM fl_billing.provider_bindings b JOIN fl_billing.records r ON r.tenant_id=b.tenant_id AND r.scope=b.scope AND r.kind=b.kind AND r.id=b.storage_id JOIN fl_billing.records c ON c.tenant_id=b.tenant_id AND c.scope='durable' AND c.kind='customer' AND c.id=$10 JOIN fl_billing.stripe_customer_bindings cb ON cb.tenant_id=c.tenant_id AND cb.storage_id=c.id AND cb.local_customer_id::text=c.id AND cb.deployment_label=b.deployment_label AND cb.account_id=b.account_id AND cb.livemode=b.livemode AND cb.verified_api_version=$11 JOIN fl_billing.portal_creation_intents j ON j.tenant_id=b.tenant_id AND j.public_id=b.public_id AND j.operation_id=$12::uuid AND j.state='completed' AND j.intent_ciphertext=$13 AND j.receipt_ciphertext=$14 WHERE b.tenant_id=$1 AND b.kind='billing_portal' AND b.public_id=$2::uuid AND b.binding_id=$3::uuid AND b.storage_id=$4 AND b.provider_id=$5 AND b.deployment_label=$6 AND b.account_id=$7 AND b.livemode=$8 AND r.data->>'id'=$4 AND c.data->>'id'=$10 AND j.payload_hash=$9")
            .bind(self.tenant.id()).bind(binding.public_id.to_string()).bind(binding.binding_id.to_string()).bind(&binding.storage_id).bind(&binding.provider_id).bind(&binding.scope.deployment_label).bind(&binding.scope.account_id).bind(binding.scope.livemode).bind(&row.2).bind(customer).bind(api_version).bind(operation.to_string()).bind(&row.3).bind(&row.5).fetch_optional(&mut **tx).await.map_err(db_error)?;
        Ok(value.ok_or_else(conflict)?.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn original_five_minute_lifetime_never_extends_or_accepts_clock_reversal() {
        let start = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let end = start + Duration::minutes(5);
        let record = json!({"createdAt":start,"expiresAt":end});
        assert!(lifetime(&record, start, start).is_ok());
        assert!(lifetime(&record, start, end - Duration::nanoseconds(1)).is_ok());
        for (first, last) in [
            (start, end),
            (start, end + Duration::nanoseconds(1)),
            (start - Duration::nanoseconds(1), start),
            (start + Duration::seconds(1), start),
        ] {
            assert!(lifetime(&record, first, last).is_err());
        }
        for expiry in [
            end - Duration::nanoseconds(1),
            end + Duration::nanoseconds(1),
        ] {
            let mut bad = record.clone();
            bad["expiresAt"] = json!(expiry);
            assert!(lifetime(&bad, start, start).is_err());
        }
    }
}
