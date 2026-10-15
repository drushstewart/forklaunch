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

At least one of `checkout`, `portal`, `planRead`, `subscriptionRead`, and `paymentLinkRead` is required; omit an unselected domain.
Null, empty, unknown, or partial sections fail. The shared scope is server-owned;
no DTO may override it. When `checkout` or `portal` is selected,
`BILLING_PUBLISHER_DATABASE_URL` is the single explicit separate publisher
connection. Its role must have each selected
publication function grant, independently validated before listening. Mixed old
`BILLING_BOUND_CHECKOUT_CONFIG` and aggregate configuration fail. The existing
`bound-checkout-v1` spelling, its configuration, and legacy registry APIs remain
supported; they do not select Portal implicitly.

Library callers construct `BoundBillingConfig` and `BoundBillingV1::trusted`
with exactly the selected creation publisher capabilities (none for `planRead`, `subscriptionRead`, or `paymentLinkRead`), then call
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
an authorized bounded form of the configured TypeScript cache reread. Subscription and PaymentLink route integration
remain separate gaps; Plan has only the explicitly selected read capability below.


### Explicit Plan provenance reads without a publisher

Set `BILLING_IDENTITY_PROFILE=bound-billing-v1` with, for example:

```json
{"deploymentLabel":"your-deployment","accountId":"acct_your_account","livemode":false,"planRead":{"trustedProducts":["your-product-id"]}}
```

`planRead` requires a nonempty bounded product allowlist and adds only
`getBoundCreatedPlan`, GET `/bound-plans/:publicId`. It does not select Plan
creation, mutation, listing, or publisher authority. A Plan-only selection rejects
`BILLING_PUBLISHER_DATABASE_URL`; combining it with Checkout or Portal creation
requires that connection and the corresponding separate publication grants.
Unknown, null, partial, or mixed old/new configuration fails before listening.

Startup checks the local direct Stripe client configuration, schema USAGE,
SELECT on `fl_billing.records`, `provider_bindings`, and `plan_creation_intents`,
and INSERT on `sensitive_read_audit`. It makes no provider request and neither
creates nor assumes publication privileges. Missing permissions, base/managed
transport, or backup fallback fail. Revoked grants after startup still fail reads.
HTTP/MCP and generated artifacts/both SDKs use the same selected registry.

The getter requires the authenticated actor's completed direct Plan journal,
the original normalized product allowlist, and an unchanged current durable
Plan DTO/alias. It verifies account/mode and expanded Plan/Product identity and
terms, then returns the original public UUID/local fields with fresh Product
presentation in `stripeFields`. No mutable `externalId`, cache entry, or raw
administrative alias supplies missing provenance. Only read audits are written;
no provider POST, retry, or compensation occurs. Data linearizes at the final
matching database SELECT; provider context expiry is rechecked after audit
COMMIT. Later remote/local changes remain possible, and no payment or
entitlement claim follows.

The existing 66 legacy routes remain available unchanged: selecting only this
new read capability does **not** make the entire generated service read-only.
Its internal HMAC service key/tenant authority is the same existing boundary,
not new end-user or cross-service IAM ownership proof.


## Direct Subscription provenance read

The same `bound-billing-v1` aggregate accepts an explicit read capability:

```json
{"deploymentLabel":"your-deployment","accountId":"acct_your_account","livemode":false,"subscriptionRead":{"plans":[{"localPlanId":"11111111-1111-4111-8111-111111111111","creationOperationId":"22222222-2222-4222-8222-222222222222"}],"collectionPolicy":"automatic_default_incomplete_v1"}}
```

This adds `getBoundCreatedSubscription`, `GET /bound-subscriptions/:publicId`,
through the shared HTTP/MCP/artifact registry. Read-only selections require no
publisher connection and reject a stray publisher setting; mixed creation selections
retain their separate publisher requirements. Startup validates direct transport
and local schema usage, SELECT on records, provider bindings, Customer bindings,
Subscription and Plan creation journals, and INSERT on sensitive-read audit. Startup
makes no provider requests. This does not remove the existing 66 legacy routes or
make the whole service read-only.

The authenticated actor must own the completed Subscription receipt. The trusted
Plan catalog may have a different actor. Current encrypted Customer/Plan records,
bindings, and completed Plan provenance must match sealed creation dependencies;
external Party association remains an administrative assertion, not IAM ownership
proof. Fresh authenticated Customer, Plan, and Subscription reads must match the
finite original terms. Only unchanged active/incomplete or incomplete-to-active
states qualify; other provider changes require another workflow. The response
refreshes `stripeFields` only: local UUID, lifecycle, party and product fields remain
unchanged. There is no payment or entitlement claim, provider POST, creation alias
fallback, or automatic lifecycle synchronization. The data snapshot linearizes at
the final matching database SELECT; context freshness is checked after audit commit.
Later local or provider changes remain possible.


### Direct completed PaymentLink reads without a publisher

An explicit `paymentLinkRead` capability uses the same aggregate selector:

```json
{"deploymentLabel":"your-deployment","accountId":"acct_your_account","livemode":false,"paymentLinkRead":{"trustedPrices":["price_your_reference"]}}
```

It adds `getBoundCreatedPaymentLink`, GET `/bound-payment-links/:publicId` to the
shared HTTP/MCP/artifact registry. Read-only selections reject a publisher setting;
mixed creation selections still require their existing separate publisher grants.
Startup checks direct local Stripe configuration, schema USAGE, SELECT on records,
provider bindings and payment-link creation intents, and sensitive-read audit INSERT.
No provider request occurs during startup. Managed/base/backup transports, partial
or unknown options, and absent grants fail before listening.

The authenticated actor must own a completed direct creation journal. The original
normalized trusted price set, current local receipt/body and alias scope/generation
must match. Administrative-only aliases and managed creation envelopes do not qualify.
An authenticated exact id/kind/mode provider read refreshes `stripeFields`; local
amount/status and other fields stay unchanged, even when provider `active` is false.
No line-item, price, paid/entitlement, cache, creation, or mutation authority is inferred.
Read evidence is bounded and provider freshness is checked after the final audit commit;
data linearizes at the last matching SELECT and can change afterward. The existing
66 legacy routes remain available unchanged; this is not whole-service read-only mode.
