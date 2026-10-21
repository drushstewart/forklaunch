# Fieldnotes identity and key configuration

The private auth service uses Better Auth 1.7.6 with PostgreSQL. Each notes request validates the actual session and current workspace membership through a fixed, service-authenticated endpoint. Client identity headers are ignored. The internal session check disables cookie caching and refresh; the public session/workspace route handles refresh cookies. Revocation is checked at admission, not atomically with a later notes commit.

Titles, bodies, insights, and queued job bodies are encrypted using AES-256-GCM with a fresh random 96-bit nonce. New writes use the tenant-derived v3 envelope described below; retained v1/v2 envelopes remain readable when the corresponding legacy key is configured. All formats bind the trusted workspace, resource and purpose through authenticated associated data. Trusted expected identity is supplied separately to decrypt; ciphertext cannot select its own tenant. Keys and decrypted job buffers are zeroized on drop. Returned note strings necessarily live as application plaintext; they have no Debug implementation. See the retained-data key rotation procedure below.

The raw service secret and session cookie never enter application log messages. The local Compose network uses HTTP internally; deploying beyond this local proof would need a reviewed transport and key-management configuration. Do not expose auth or worker ports independently.

## Retained-data key rotation

API and worker use `EnvelopeKey::from_env()`. New writes use authenticated
`v3.<key-id>.<nonce>.<ciphertext-and-tag>` envelopes. The key ID is also part of
AES-GCM associated data, alongside workspace, resource and purpose. Unknown IDs
fail closed; readers never try every key. `EnvelopeKey::new` remains the explicit
v1 compatibility constructor. Key storage is private and zeroized on drop.

`ENCRYPTION_KEY_HEX` is the active 32-byte key as 64 hex characters;
`ENCRYPTION_KEY_ID` defaults to `primary`. `ENCRYPTION_PREVIOUS_KEYS_JSON` maps up
to four previous IDs to hex keys. IDs contain 1–32 ASCII letters, digits, `_` or
`-`. Empty optional environment values behave as absent. The original unkeyed
v1 format uses `ENCRYPTION_LEGACY_KEY_HEX`: absent/empty defaults to the active
key for initial adoption; `disabled` explicitly rejects v1. This fallback is
only for v1 envelopes, never unknown v2 IDs.

To rotate without losing retained data:

1. Preserve the original v1 key explicitly in `ENCRYPTION_LEGACY_KEY_HEX` if any
   v1 notes, attachments, cached records or queued jobs remain.
2. Put the former active ID and key into `ENCRYPTION_PREVIOUS_KEYS_JSON`. Generate
   a fresh key and unused ID, and set them as the active pair on both API and
   worker. Restart both together; a writer with the new key must not run while
   another reader lacks it.
3. Read retained notes and attachments and process retained jobs, then create
   and read new data. New writes use the new ID; old ciphertext is not silently
   rewritten. Redis records can simply expire, but database rows, object blobs,
   pending jobs and backups require their historical keys.
4. Remove an old key only after its retained ciphertext has been explicitly
   migrated or deleted, including backups. This prototype does not provide a
   bulk re-encryption command. Keep old keys through rollback windows; reverting
   to a binary/configuration that cannot read v2 or its active ID loses access.

The rotation tests retain v1 and old-ID v2 data while writing a new ID, reject
removed/wrong keys and ID substitution, and independently verify Node/Rust v1
and v2 AES-GCM vectors. These cryptographic checks do not replace an operational
retained-data check on the generated application.

Retirement is not immediate erasure: reading old database ciphertext while its
key is available may create a cache entry encrypted under the current active
key. Such a record remains readable until the cache entry expires (60 seconds)
or is removed, even after its database key is retired. Operational retirement
checks must expire/remove the relevant cache entries before asserting that the
underlying retained ciphertext is unreadable. Auth and database generation
checks still precede every cache hit.

## Responsibility split

This package now contains only the application's workspace-role projection,
nonforgeable `VerifiedTenant`, auth-service composition, and key environment
configuration. `forklaunch-core::auth::session_client` owns fixed-endpoint bounded
session-verification transport; it does not know workspace roles or database
queries. `forklaunch-encryption` owns authenticated envelopes and accepts an
explicit namespace profile. Fieldnotes chooses its existing `fieldnotes` wire
namespace, preserving all retained v1/v2 ciphertext.

The `fieldnotes-notes` service owns notes, outbox, events, encrypted
attachments, cache and SQLx/RLS mapping through `NotesRepository`. It composes the
reusable primitives; those business operations are not framework security APIs.
The SQLx-to-`SecurityError` conversion remains a compatibility adapter while the
application error types are shared.

## Project tasks

Run from this project directory:

```sh
make fmt-check
make lint
make check
make test
make build
```

`make fmt` formats only this Cargo package. `lint` runs Clippy for all package
targets with warnings treated as errors. Override the Cargo command when needed:
`make CARGO='cargo +stable' test`. These targets use the local `Cargo.toml`; the
application workspace still coordinates shared dependencies and its lockfile.

### Tenant-derived writes

Version 3 derives a separate 256-bit AES key per workspace using HKDF-SHA256. The salt is `forklaunch-envelope`, and the UTF-8 info is `fieldnotes:v3:<workspace-uuid>`. Associated data binds namespace, version, key ID, workspace, resource and purpose. Old v1/v2 data is authenticated using its original format; new writes never downgrade. Deploy compatible readers before enabling new writers across independently rolled services. This wire format is distinct from the TypeScript persistence envelope; shared cryptographic guarantees do not imply byte compatibility.
