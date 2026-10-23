---
"@forklaunch/implementation-messaging-base": patch
"@forklaunch/implementation-messaging-twilio": patch
---

SMS services no longer `await` MikroORM's synchronous `persist` inside their transactions. No behaviour change.
