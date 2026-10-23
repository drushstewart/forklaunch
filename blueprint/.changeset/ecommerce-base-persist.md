---
"@forklaunch/implementation-ecommerce-base": patch
---

Cart, inventory, order, payment, product and variant services no longer `await` MikroORM's synchronous `persist` inside their transactions. No behaviour change; the flush still happens when the transaction commits.
