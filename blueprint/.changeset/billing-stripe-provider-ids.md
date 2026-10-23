---
"@forklaunch/implementation-billing-stripe": patch
---

Payment-link update, expire, success and failure, and checkout-session expire, success and failure, now call Stripe with the Stripe id stored on the record (`stripeFields.id`, `plink_...` or `cs_...`) instead of the record's public id. The generated billing-stripe module gives records a local UUID, so these calls used to send Stripe that UUID and failed against the real API with "No such payment_link" or "No such checkout.session". The public id is unchanged in the API.

The services now read the cached record before calling Stripe. A missing record fails before any Stripe call, and so does a record without a stored Stripe id: if the record has no stored provider object, its own id is used only when that id is itself a Stripe id. `updatePaymentLink` also rejects a Stripe response whose id differs from the stored id. A custom `UpdatePaymentLinkMapper.toLocalUpdate` receives the Stripe response, whose `id` is now the Stripe id rather than `dto.id`.
