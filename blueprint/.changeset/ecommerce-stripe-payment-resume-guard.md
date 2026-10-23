---
"@forklaunch/implementation-ecommerce-stripe": minor
---

`StripePaymentService` resuming a payment checks that Stripe returned the PaymentIntent the payment references, and throws otherwise instead of handing back another intent's client secret. Payment mappers can supply an optional `PaymentMapper.validateResume(payment, em)` guard, which runs before and after the secret is retrieved (for example to refuse a payment whose order was cancelled). Mappers without it behave as before.
