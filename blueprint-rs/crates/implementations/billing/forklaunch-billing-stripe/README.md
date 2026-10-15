# Stripe billing

`StripeBillingClient::verify_context(expected_account, expected_livemode)` verifies a direct account using authenticated `GET /v1/account` and `GET /v1/balance`. Expectations must come from trusted server configuration. The account response must match the expected account ID and the balance must explicitly report the expected boolean mode. No account or mode is inferred from API key prefixes. Restricted keys without both read permissions fail closed. Requests use the client's existing bounded HTTPS/trusted-proxy transport and pinned API version; no Connect account header or provider mutation is added.

The returned `VerifiedStripeContext` has no public constructor, clone or serializer. `ensure_current` requires the same immutable client generation and less than five seconds elapsed since verification started. A cloned client retains the generation; a newly constructed client or version reconfiguration requires verification again. Future binding lookups/provider mutations must obtain fresh verification for each operation and recheck immediately before use. This observed context is not a financial-operation capability, proof of local ownership, or guarantee that a remote credential/proxy has not changed after the reads. Existing legacy billing trait methods are not gated by this API.

Managed verification uses the companion's signed JSON `POST /payments/context` endpoint. Its account/mode/mapping-generation/API-version snapshot and opaque token are bound to the configured instance, immutable platform credential/API base and five-second lifetime. `retrieve_checkout_in_context` submits that token to `/payments/context/checkout-read`; `retrieve_payment_link_in_context` uses `/payments/context/payment-link-read`; the platform pins the same account, rechecks mode/generation/expiry, and returns a matching scope envelope. Native code checks its client generation and elapsed time again. Old gateways without the protocol fail closed. The raw Stripe allowlist still excludes account and balance: those are internal verification reads only. The companion generation migration is required; credential/base rotation and account remapping invalidate outstanding contexts. These read observations grant no financial mutation capability.

Provider definitions: [account retrieval](https://docs.stripe.com/api/accounts/retrieve), [balance retrieval](https://docs.stripe.com/api/balance/balance_retrieve), and [balance mode](https://docs.stripe.com/api/balance/balance_object). The repository's pinned official Stripe SDK uses `/v1/account` for `accounts.retrieve()` without an ID, and `/v1/balance` for `balance.retrieve()`; the owned SDK/native oracle checks both requests.

With `postgres-identity`, `StripeBillingService<PostgresBillingStore>::get_bound_checkout_session` is an opt-in durable read by public UUID. It verifies direct or managed account/mode, resolves an explicitly provisioned tenant/deployment/account/mode checkout binding, retrieves only its provider reference, then joins the exact binding generation to the encrypted durable row and audits decryption. Freshness is checked after network and database work. Provider object, ID, subscription mode, live mode and customer must match; the customer must match the durable local customer relation. Expanded customers require the customer object type and an absent or false boolean deletion flag. A replaced alias, missing ownership, malformed evidence or unavailable audit fails closed. The returned DTO uses the public UUID and fresh provider fields; storage keys and ciphertext remain unchanged.

This method requires existing administrative durable bindings and the base billing grants, including sensitive-read audit INSERT. Its joined record/binding query provides a statement snapshot, not a lock or capability for later financial operations. It performs no provider mutation, automatic backfill, new-record UUID migration or HTTP route wiring; cache-only and existing trait reads remain separate. Portal reads and mutation integration remain unsupported by this opt-in read; the separate payment-link API is described below.

`get_bound_payment_link` adds an opt-in direct or managed durable payment-link read under `postgres-identity`. It uses the same explicitly provisioned tenant/deployment/account/mode alias and exact-generation audited encrypted-row read, and verifies `payment_link` object/ID/live mode under a fresh context. It returns the public UUID and fresh provider fields while preserving local amount/status and storage ciphertext. Payment links have no configured local customer relation to compare. Managed clients require the dedicated signed `/payments/context/payment-link-read` endpoint and exact matching scope envelope. Old gateways fail closed. The default service still uses provider IDs; this API does not automatically convert existing callers, provision aliases, add HTTP routes, or authorize mutations. Context expiry after database waits also denies the result.

With `postgres-reconciliation`, `acknowledge_already_paused_effect` is an explicit operator workflow for `customer.subscription.paused`. It verifies the configured account/mode, retrieves the exact pending event and current subscription with authenticated GET requests, and requires both provider objects to be paused with the exact sealed binding/customer identity. The owned local subscription must already have `active=false`; exact row/binding versions and pending-attempt identity protect the atomic audit/receipt update. No subscription/provider write, event replay, financial action, or public route is added. Managed transport uses the dedicated signed `/payments/context/paused-subscription-read` endpoint, which retrieves the event and subscription under the same pinned account/API version and checks binding generation and expiry after every await. Old gateways fail closed. The returned scope must exactly match the verified context. An event `account` may be absent/null under this authenticated retrieval scope; when present it must match. Non-null event `context` is unsupported. This is authenticated GET evidence, not proof of the original webhook HMAC or a causal pause epoch.

Authenticated GET evidence is not verification of the original webhook HMAC. Stripe exposes no pause-generation token: matching old-event/current paused state does not prove an uninterrupted pause or that the interrupted callback caused the local state. This API only acknowledges an already-satisfied effect. Event creation must precede the stored attempt and inspection time. [Stripe event retrieval](https://docs.stripe.com/api/events/retrieve) supports events created within the last 30 days; older/unavailable events, missing bindings, changed customers and contradictory state remain manual review. Paused status is distinct from merely pausing collection.

The same immutable client context has a five-second verification window checked after provider and database awaits, including immediately before COMMIT. A late post-COMMIT check returns `RecordedEvidenceNoLongerCurrent`, explicitly recording that a receipt was committed. Actual COMMIT timing is not guaranteed; timeout/cancellation/COMMIT errors require the store's read-only receipt query. See the base persistence guide for additional explicit trusted-operator locking/audit permissions.


## Verified direct creation

The explicit PostgreSQL creation APIs retain public UUIDs separately from Stripe
references. `create_bound_plan` verifies an approved Product and journals one Plan
creation. `register_verified_stripe_customer` verifies Customer existence/account/
mode and records a separately server-authorized local Customer/Party association.
Neither email equality nor provider metadata proves the business association.
Party authorization and lifecycle in another service remain application-owned.

`create_bound_subscription` requires that Customer registration, a completed
unchanged bound Plan creation, an explicit operation UUID and a trusted profile:

```rust,ignore
let profile = BoundSubscriptionCreationProfile::trusted(
    scope,
    vec![(local_plan_id, completed_plan_operation_id)],
    SubscriptionCollectionPolicy::AutomaticDefaultIncomplete,
)?;
let publisher = store.trusted_subscription_publisher(publisher_pool).await?;
let outcome = service.create_bound_subscription(
    &profile,
    &publisher,
    operation_id,
    BoundSubscriptionCreateRequest {
        local_customer_id,
        local_plan_id,
        quantity: 1,
        description: None,
    },
).await?;
```

Apply the billing migrations through11 using the migration owner. Provision
separate non-owner/non-BYPASSRLS runtime and publisher credentials; runtime must
not inherit publisher or migration owner. Explicitly grant the publisher the
publication-function permission and required base-store access. The constructor
checks role separation and matching configured/observed database identity; it
never falls back to runtime credentials. Publisher credentials are trusted server
authority and must stay outside request data. The SQL function and native caller
share one transaction, including post-await freshness checks before COMMIT.

The finite collection policy selects `charge_automatically` and
`default_incomplete`, with one verified licensed/per-unit Plan. Fresh exact
Customer/Plan and Subscription evidence must match. `active` means observed
subscription service state; it does not establish a paid invoice or charged total.
The profile rejects unsupported trial, discount, tax, delegation and ambiguous
item/evidence states. It adds no default HTTP route or managed creation support.

Confirmed reservation holds durable claims on Customer/Plan bodies and binding
generations before the single provider POST. Successful publication writes UUID,
ownership binding, audit and encrypted receipt before releasing those claims.
`Unresolved` retains the operation for explicit inspection and never retries the
provider write. `RecordedEvidenceNoLongerCurrent` means a receipt was committed
before evidence expired. `inspect_bound_subscription_creation` returns historical
pending/completed state without dispatch authority or a current-payment claim.
General recovery and ownership-aware mutations remain separate work.
