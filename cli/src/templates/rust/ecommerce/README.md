# {{{name}}}

Persistent ecommerce service with HTTP and MCP, HMAC catalog/cart/order/payment routes, bounded catalog pages, Stripe/optional PayPal payment creation and an inventory worker.

Apply `cargo run -p {{{name}}} -- --migrate` with a schema-owner connection. Run the HTTP service with `cargo run -p {{{name}}}` and a separate inventory process with `cargo run -p {{{name}}} -- --worker`. HTTP health is `/health`; MCP is `/mcp` on the same listener. Offline `FORKLAUNCH_MODE=contract` exports routes without provider/storage credentials. Configure the trusted tenant, encryption keys and providers in `.env.example`; request fields cannot change the database tenant.

Runtime database roles must have no SUPERUSER/BYPASSRLS. Grant schema USAGE on `fl_ecommerce,fl_worker`, SELECT/INSERT/UPDATE/DELETE on `fl_ecommerce.records,fl_worker.jobs`, SELECT/INSERT on `fl_ecommerce.inventory_event_receipts`, INSERT on `fl_ecommerce.pii_read_audit`, and USAGE on `fl_worker` sequences. Apply migrations separately with the owner role. Addresses are encrypted and plaintext reads audit the verified service actor.

Order transitions and their inventory jobs commit together. The worker decrements on paid, restocks cancellation from paid/fulfilled and ignores stock for pending cancellation. All item deltas and durable receipts commit together, preserving stock on retry/redelivery. The worker uses the same tenant and database, with optional HTTP readiness and bearer-authenticated pause/resume/drain controls.

Connect charges default to zero platform fees, matching the TypeScript module. Client secrets appear only in Stripe creation responses. `STRIPE_API_BASE` accepts HTTPS proxies or explicit loopback fixtures. PayPal requires its client ID/secret/base URL together.

This template covers the core 27 application routes. Checkout/tax/shipping/import, provider webhook reconciliation and compliance controllers remain tracked in the Rust parity plan; native database tables are not a shared MikroORM schema. Add custom HTTP routers through `routes::contracts()` and `routes::register_router(...)` and regenerate both SDKs after changing contracts.
