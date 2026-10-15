# Billing behavior and interruption audit

This finite audit distinguishes configured TypeScript differences from identical
legacy behavior and stronger reconciliation requirements. It is not a blanket
billing completion receipt. Paths below are repository-relative.

## A. Confirmed configured TypeScript / native differences

| Behavior | Actual source evidence | Native status / required proof |
| --- | --- | --- |
| Subscription update owner preflight | `blueprint/billing-stripe/domain/mappers/subscription.mappers.ts`, `UpdateSubscriptionMapper.validateUpdate`, rejects changed `partyId` or `partyType` before the provider GET/POST in `blueprint/implementations/billing/stripe/services/subscription.service.ts` | `blueprint-rs/crates/implementations/billing/forklaunch-billing-stripe/src/services.rs`, `update_subscription`, previously sent provider calls and merged owner patches. The correction selects the configured guard in generated Stripe services while preserving the generic/custom constructor. Actual source84 cases, native service2 tests and restricted-PG HTTP dispatch validate this narrow owner-preflight correction; the generic170-operation regression remains unchanged. |
| Public IDs and retained provider references | Configured application mappers allocate local UUIDs. Configured Checkout and PaymentLink read hooks resolve retained provider references; PaymentLink list validates the whole local selection before sequential provider reads. | Generic native creation stores provider IDs for these families; generic reads use their IDs directly. Explicit created-provenance APIs provide a tested subset, not automatic equivalence of every default route/cache configuration. |
| Configured list selection | `blueprint/implementations/billing/stripe/services/paymentLink.service.ts`, configured `listPaymentLinks`, supports cache selection and bounded explicit-ID reads. | The bound explicit-ID list is implemented. No-ID/empty-ID cache enumeration remains outside that profile; generic native account-list behavior is a separate legacy path. |

The owner-preflight source fixture also distinguishes omission from explicit
JavaScript `undefined`. Omission and unchanged values succeed. `null` is rejected
before provider I/O. Own-properties containing `undefined` pass the owner check
but fail the actual later mapper after provider mutation and after local
assignment/persistence. JSON HTTP/MCP cannot express this own-property state;
native `Option::None` represents omission, not an exact model of that JS-only
failure sequence. Typed native deserialization rejects explicit null. HTTP schema
admission may reject malformed input before a missing-row lookup; this audit does
not claim identical internal error ordering for malformed transport input.

## B. Matching baseline operations, not missing Rust capabilities

| Operation | TypeScript and native behavior |
| --- | --- |
| Checkout expiration / status updates | `checkoutSession.service.ts`, `expireCheckoutSession` / `handleCheckoutSuccess` / `handleCheckoutFailure`, and native `services.rs` forward the supplied ID, perform the provider operation first, then change local state. Raw UUID dispatch is a shared limitation, not itself a port discrepancy. |
| PaymentLink metadata status updates | `paymentLink.service.ts`, expiration/success/failure, and native counterparts update provider metadata before local changes. Metadata expiration is not provider deactivation. |
| Plan replacement / deletion | `plan.service.ts`, `updatePlan`, and native `update_plan` retrieve, delete the old plan, create its replacement, retrieve expanded data, then write locally. Deletion is provider-first in both. |
| Subscription lifecycle | `subscription.service.ts`, update/cancel/delete/resume, and native counterparts perform provider effects before local state changes. Existing cancellation is not an implemented provider pause/resume cycle. |
| Portal replacement | `billingPortal.service.ts`, `updateBillingPortalSession`, and native counterpart create a new provider session before local replacement. |

The extracted Checkout expiration characterization covers both UUID and Stripe-
style IDs, provider failure, missing cache, delete failure and success. A missing
cache can fail after remote expiration. Its owned collaborators are not a real
Redis/ORM integration claim.

## C. Explicit checklist reconciliation requirements

The checklist's interrupted-operation and cancelled-order gates require stronger
behavior than the shared provider-first baseline:

- Unknown creation outcomes for Plan, Checkout, Portal and PaymentLink need
  operation-specific evidence or manual inspection. Subscription event-linked
  creation recovery and managed PaymentLink receipt inspection cover only their
  declared profiles.
- Plan replacement can fail after deleting the old provider plan. No generic
  retry can safely reconstruct that transaction.
- Checkout webhook local repair and paused-subscription bookkeeping acknowledgement
  do not cover every webhook effect or prove all lifecycle causality.
- Captured/cancelled-order reconciliation remains restricted to the explicitly
  verified Stripe/PayPal evidence profiles. Ambiguous, paid, authorized, refunded,
  disputed, or omitted histories must not be silently repaired.

A Checkout expiration journal would address this category, not add a missing
legacy expire method. It is deferred while confirmed configured mismatches are
prioritized. Stripe's official expiration endpoint accepts open sessions and can
reject sessions that already expired or completed; an observed expired GET alone
does not prove which operation caused expiration.

## D. Additional authority and hardening

Immutable aliases, separate publishers, durable single-dispatch claims and fresh
account/mode proofs provide explicit safer authority models. They are necessary
when introducing usable UUID/provider adapters or the checklist's recovery
features, but are not independently proof that an upstream operation was absent.
No automatic alias provisioning from mutable cache/provider fields, financial
retry, or blanket five-family closure follows from this audit.

Source characterization: `cli/tests/rust_billing_subscription_update_preflight.mjs`
and `cli/tests/rust_billing_checkout_expiration_source.mjs`.
Primary provider reference: <https://docs.stripe.com/api/checkout/sessions/expire>.

## Owner-preflight correction validation

- `configured_subscription_update` in billing-stripe tests configured rejection
  before provider calls/local writes, unchanged or omitted owner acceptance,
  missing-row ordering, null DTO rejection, and generic/custom reassignment.
- The same-named billing-axum target runs actual HMAC HTTP against restricted
  PostgreSQL: changed owners return sanitized 400, omitted/same owners return
  200 after GET/POST, and missing rows return 404 without provider access.
- `rust_billing_bound_checkout_artifacts.mjs` renders and compiles both generated
  main templates and asserts only Stripe selects the configured guard.
- The actual TS source fixture records the JS-only explicit-undefined boundary;
  it does not falsely equate that state with JSON omission.

This closes the specified owner-update mismatch only. Sections A's remaining
identity/list gaps and C's interruption gates remain separate work.

The configured update schema declares `id: string`, not UUID. The source matrix
runs both UUID and non-UUID IDs. The native configured guard also validates the
supplied `billingProvider` (`stripe`) and `partyType` (`user`/`organization`)
enums; these are already constrained by generated HTTP schema admission. Its
blank check uses the exact ECMAScript whitespace set: retained U+0085 is not
blank, while U+FEFF and U+2028 are. No arbitrary provider-options schema parity
claim is added by this narrow preflight correction.

The HTTP receipt establishes native dispatch, status and sanitization only. The
actual configured TypeScript mapper throws an ordinary Error; this batch does
not run the TypeScript HTTP adapter and does not establish cross-language HTTP
error-envelope/status parity. That broader transport gate remains explicit.
