---
'@forklaunch/core': patch
---

`stripeGatewayFetch` keeps Bun's `fetch.preconnect` (pointed at the gateway origin) when the runtime has it, so the Stripe SDK works on Bun. The `forklaunch-gateway-mock` binary adds the scoped payments context routes (`POST /payments/context`, `/payments/context/checkout-read`, `/payments/context/payment-link-read`, `/payments/context/paused-subscription-read`) that managed billing uses for verified checkout, payment-link and paused-subscription reads.
