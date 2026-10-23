---
"@forklaunch/implementation-iam-base": patch
---

- `updateBatchPermissions` resolves each permission's roles on the transaction's EntityManager, one at a time, and flushes inside the transaction. It used to start the updates without awaiting them, so the batch could persist before its role changes were read.
- `updatePermission` passes the caller's EntityManager through to role resolution.
- `getBatchUsers` and `deleteBatchUsers` filter by `id: { $in: ids }` and no longer pass the `ids` array itself as a filter field.
- Services stop awaiting MikroORM's synchronous `persist`.
