# Billing HTTP and MCP profiles

The default `BillingBackend::base` and `BillingBackend::stripe` retain the existing
billing routes and identifier behavior. Request date normalization is separately
selected with `BillingRequestDateProfile`.

## BoundCheckoutV1

The optional `bound-checkout` feature provides three additional internal,
request-HMAC-authenticated operations. HTTP and MCP use the same handlers and
`bound_checkout_contracts()` registry; generated contract/OpenAPI/SDK artifacts
select that same registry. Existing billing routes remain unchanged.

| Operation | HTTP route | Meaning |
| --- | --- | --- |
| `createBoundCheckoutSession` | POST `/bound-checkout/operations/:operationId` | One guarded creation attempt with visible caller operation UUID |
| `getBoundCreatedCheckoutSession` | GET `/bound-checkout/sessions/:id` | Provenance-aware read by published local UUID |
| `inspectBoundCheckoutOperation` | GET `/bound-checkout/operations/:operationId` | Actor-owned historical IDs/state; no URL or dispatch capability |

The create body has exactly `localCustomerId`, `localPlanId`, and `quantity`.
Both IDs are non-nil UUIDs; quantity is an integer from 1 through
9,007,199,254,740,991. No caller `stripeFields`, provider ID, redirect, account,
fee, or delegation option is accepted. The Customer must already have a trusted
registered provider relation; the Plan must have a completed bound creation
operation authorized by server configuration. Neither cache nor mutable provider
fields can establish these relations. An authenticated internal service key is
trusted to select business records within the configured tenant; this is not
per-end-user authorization or verification of an external IAM Party lifecycle.

For generated Stripe billing services, explicitly set all three variables:

- `BILLING_IDENTITY_PROFILE=bound-checkout-v1`
- `BILLING_BOUND_CHECKOUT_CONFIG` to the finite JSON configuration below
- `BILLING_PUBLISHER_DATABASE_URL` to a distinct, explicitly trusted publisher role

```json
{
  "deploymentLabel": "production",
  "accountId": "acct_CONFIGURED",
  "livemode": true,
  "plans": [{"localPlanId": "LOCAL-PLAN-UUID", "creationOperationId": "COMPLETED-PLAN-OPERATION-UUID"}],
  "successUrl": "https://your-app.example/success",
  "cancelUrl": "https://your-app.example/cancel"
}
```

The UUID placeholders must be replaced with real provisioned UUIDs. Configuration
is bounded to 64 KiB, rejects unknown fields, and is not included in errors or
SDK metadata. `TENANT_ID` remains the fixed server tenant. Partial configuration,
unknown profiles, base/managed transports, and `BILLING_DATABASE_BACKUP=true`
fail startup. Legacy mode rejects stray bound-profile settings. The generated
library retains its legacy registry APIs and additionally exports
`contract_registry_with_profiles` for explicit artifact selection.

Library applications construct `BoundCheckoutConfig`, an explicitly validated
`TrustedCheckoutPublisher`, and `BoundCheckoutV1::trusted`, then await
`BillingBackend::with_bound_checkout` **after** selecting backup settings.
Publisher actor derivation retains the original validated pools and role/database
boundary. Every request derives its actor from the verified HMAC principal and
validates publication against that request's runtime store. Never expose actor
selection or publisher credentials to request DTOs.

Apply billing migrations through 15 with a migration role. Runtime and publisher
must be distinct non-superuser, non-BYPASSRLS roles; runtime must neither inherit
the publisher/migration owner nor execute `publish_bound_checkout`. The publisher
is trusted authority and needs explicit EXECUTE on that publication function.
Follow the billing base/Stripe creation-journal grant requirements for records,
Customer/Plan bindings, Checkout intents/claims, and audit tables. Runtime needs
journal SELECT for reads/inspection and the documented reservation privileges;
read audits must be writable. Private dependency guards and completed-transition
RLS remain enforced even with broader table grants. No automatic grants or
migrations run at startup.

A fresh successful creation returns HTTP 200 with `state: "created"`, operation
and public UUIDs, and the DTO. A completed duplicate returns HTTP 200 with
`state: "completed"` and IDs only. Unknown dispatch/COMMIT and expired post-write
observations return HTTP 202 with the operation UUID and explicit pending or
`recordedEvidenceNoLongerCurrent` state. Inspect before deciding further action;
a missing receipt never authorizes provider retry. Existing pending operations
retain dependency claims and never automatically dispatch again. Inspection does
not return a historical URL as if refreshed. The separate GET returns the stored
DTO with current verified provider fields and its original expiry, not a newly
issued session URL.

Creation is neither payment nor entitlement proof. Provider observations are
bounded in time and do not lock remote configuration. The finite direct-only
creation profile and its separate publisher transaction retain their existing
ambiguity/recovery boundaries. This namespace does not switch all five billing
families to configured TypeScript parity and adds no financial mutation alias
resolution, account-wide list scanning, or automatic legacy data migration.

## BoundBillingV1 aggregate

Select `BILLING_IDENTITY_PROFILE=bound-billing-v1` and supply
`BILLING_BOUND_BILLING_CONFIG` with one shared scope and explicitly selected domains:

```json
{
  "deploymentLabel": "production",
  "accountId": "acct_CONFIGURED",
  "livemode": true,
  "checkout": {
    "plans": [{"localPlanId": "LOCAL-PLAN-UUID", "creationOperationId": "COMPLETED-PLAN-OPERATION-UUID"}],
    "successUrl": "https://your-app.example/success",
    "cancelUrl": "https://your-app.example/cancel"
  },
  "portal": {
    "configurationId": "bpc_CONFIGURED",
    "returnUrl": "https://your-app.example/account"
  }
}
```

At least one of `checkout` and `portal` is required; omit an unselected domain.
Null, empty, unknown, or partial sections fail. The shared scope is server-owned;
no DTO may override it. `BILLING_PUBLISHER_DATABASE_URL` remains the single
explicit separate publisher connection. Its role must have each selected
publication function grant, independently validated before listening. Mixed old
`BILLING_BOUND_CHECKOUT_CONFIG` and aggregate configuration fail. The existing
`bound-checkout-v1` spelling, its configuration, and legacy registry APIs remain
supported; they do not select Portal implicitly.

Library callers construct `BoundBillingConfig` and `BoundBillingV1::trusted`
with exactly the selected independent publisher capabilities, then call
`with_bound_billing`. Artifact callers use `contract_registry_with_bound_billing`
with the same configuration; a selector alone cannot guess selected families.
The existing optional `bound-checkout` Cargo feature also enables this additive
aggregate. No new dependency, migration, or automatic publisher grant is needed.

Portal adds `createBoundPortalSession` (POST) and `inspectBoundPortalOperation`
(GET), both at `/bound-portal/operations/:operationId`. The body contains exactly
`localCustomerId`, a registered local Customer UUID. The server pins configuration
and return URL. HTTP, MCP, and generated SDKs share these contracts and the
same trusted service-key/tenant authority boundary described above. Creation uses
the existing Portal journal, customer claims, direct provider validation, and
separate trusted publisher. Fresh creation may return a URL; completed duplicates
and inspection return IDs only. Uncertain effects remain pending, without retry
or claim release. These routes confer no payment or entitlement proof.

`getStoredBoundPortalSession` adds GET `/bound-portal/sessions/:publicId` under
the same authenticated actor and server policy. It returns
`{state: "stored", observedAt, record}` without provider GET or POST. This is the
original stored DTO, not a new or refreshed session. An actor-owned completed
receipt, matching current Portal record and alias, and unchanged registered
Customer body/binding are all required. No generic cache or mutable provider
field can establish authority. Changed server configuration, scope/API version,
missing provenance, and managed transport fail closed.

The sealed original `expiresAt` must be exactly five minutes after its original
`createdAt`. Reads reject future creation times, backward clocks, and expiry,
including expiry during audit/database waits; expired reads return HTTP 410
without a URL. No TTL is extended. The data snapshot linearizes at the final
matching SELECT after audit waits. `observedAt` records the subsequent completion-
clock TTL check after audit COMMIT; it does not mean data remained unchanged
through COMMIT or afterward. Current alias generation is checked across this
read, without claiming the alias was never administratively rebound historically.

The stored URL may already be invalid remotely. Its local five-minute policy
proves neither remote validity nor payment/entitlement state. Creation duplicates
and operation inspection still return IDs only, even when a separate explicit
stored-read operation is permitted. This preserves that separation while adding
an authorized bounded form of the configured TypeScript cache reread. Plan,
Subscription, and PaymentLink route integration remain separate gaps.
