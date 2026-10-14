# Base IAM identity integration

Directory CRUD and credential/provider users remain independent unless an authorized administrator explicitly links existing identities. `PostgresDirectoryStore::bind_auth_identity(directory_id, auth_user_id)` creates a tenant-local, one-to-one association and an actor/correlation audit in one transaction. Repeating the same pair is idempotent. Email, profile fields, passwords, organizations and grants are never copied or used to infer a link. Directory create/update mappers continue to ignore passwords.

`PostgresIamService::directory_user_for_session(token)` accepts the **complete issued session token**, including the complete JWT when RS256 issuance is configured. It hashes the token and requires a current persisted session, an enabled credential user and a current directory binding in the same tenant. A user ID, JWT subject, arbitrary session ID or independently signed JWT is insufficient. Expired, revoked, disabled and foreign-tenant sessions return `None`. This lookup supplies an identity association; applications must still authorize directory operations and resolve current directory grants.

`unbind_auth_identity(directory_id, expected_auth_user_id)` removes only the exact current association with an atomic audit. Independent credentials, provider mappings and sessions remain valid, but those sessions cease resolving a directory identity. Ordinary directory deletion, including compliance erasure, is restricted while the identity is bound. Authorized administrators can instead use `delete_linked_identity(directory_id, expected_auth_user_id)` to atomically delete the credential user, cascade all persisted sessions/provider mappings, and delete the directory identity and uniqueness claims with audits. Any failure rolls the entire operation back. Deleting a credential user through the existing API also cascades its association and sessions while retaining the independent directory profile.

These methods are administrative library APIs, not public self-service linking endpoints. The caller must authorize the administrator and construct a trusted tenant-bound store with an `AuditContext` containing a nonblank trusted actor ID and correlation ID. Missing/blank actors are rejected by these three administrative methods; ordinary directory CRUD keeps its existing audit policy. The three new administrative APIs record their mutations in the existing directory audit table. Legacy `PostgresIamService::delete_user` has no audit context: its foreign-key association cascade retains the existing unaudited deletion semantics. Resolving an association does not create an administrative audit event.

Migration `0006_directory_identities.sql` must run under the schema-owner connection. Runtime roles using the new association APIs need the following additional grant (replace `iam_runtime` with the application's restricted runtime role):

```sql
GRANT SELECT, INSERT, UPDATE, DELETE ON fl_iam_directory_identities TO iam_runtime;
```

Existing directory/auth table and audit grants remain required. The new table enforces tenant RLS and composite foreign keys; runtime roles must not have SUPERUSER or BYPASSRLS. Default CRUD/auth APIs do not query the association table. Generic custom mapper/filter behavior and automatic directory authorization integration are separate work.
