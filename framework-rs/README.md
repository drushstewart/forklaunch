# framework-rs

See the [workspace guide](GUIDE.md) for the complete crate/feature catalog, development and operations workflows, API documentation commands, and compatibility boundaries.

Rust-native ForkLaunch framework crates.

This workspace implements contract registration, validation primitives, an Axum HTTP adapter, OpenAPI and AsyncAPI generation, MCP tool generation and an optional Streamable HTTP server, SDK code generation, and infrastructure interfaces. Optional runtime integrations are feature-gated: PostgreSQL tenant transactions, TypeScript-compatible field encryption, Redis storage, and remote JWKS verification. Axum supports verified sessions, RBAC, and OpenTelemetry request instruments. Production integration and cross-language parity work remains tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md).

The CLI can generate runnable Rust backends with bundled framework source. See [Rust backend generation](../docs/adding-projects/rust.md) for setup, contract exports, SDKs, and current boundaries.

Scalar docs accept additional documents through `RouterConfig.docs_sources`.
Use `ScalarSource::url("External API", "https://example.com/openapi.json")` or
`ScalarSource::inline("Authentication", schema)`; optional `slug` and `default`
fields control source navigation. The generated application API remains first.

Application-owned HTTP APIs can use `nest_internal(prefix, router)` or
`merge_internal(router, namespaces)` for a root mount. Declare every raw namespace
when merging. Contracts, documentation and MCP endpoints cannot claim reserved
namespaces; parent router paths and shared middleware still apply. Internal
routes remain outside generated application contracts and SDK/MCP operations.

Persistence adapters can derive `forklaunch_core::persistence::compliance::ComplianceMetadata`
on named structs. Field attributes such as `#[compliance(level = "pii", nullable,
name = "email_address")]` declare classification, storage nullability and storage
name. Supported levels are `none`, `pii`, `phi`, and `pci`; unannotated fields
default to `none` and non-nullable. Only explicitly classified nullable fields
qualify for anonymization. Metadata does not turn on encryption or perform
export, erasure, or retention writes: adapters must verify their storage schema
and enforce tenant boundaries. Rust `Option`, serde renames and encryption
configuration do not implicitly change these declarations. The derive supports
generics and `#[compliance(crate_path = "renamed_core")]` for renamed dependencies.

The `persistence::retention` module provides bounded async enforcement hooks for
application-owned stores. Build a `RetentionPolicy::for_entity::<T>` from explicit
compliance metadata and an already resolved exclusive cutoff. `enforce` requires
validated tenant identities, supports dry-run counts and entity selection, and
reports failed/incomplete passes without losing earlier committed counts. Stores
must implement atomic tenant-scoped batches, oldest-first selection and storage
metadata validation; anonymization clears only classified nullable fields and
records its timestamp even when no field qualifies. Batch sizes and pass lengths
are bounded. Duplicate scopes, future cutoffs and counter overflow fail explicitly.
Calendar-duration parsing, timezone selection and concrete database adapters remain
application responsibilities. The [in-memory conformance adapter](crates/forklaunch-core/examples/retention_hooks.rs)
illustrates the hook boundary; 24 cases compare it with the actual TypeScript
retention service. This does not claim generic SQL/ORM retention parity.

## JWT authentication

`forklaunch_auth::issue_jwt_hs256(&claims, secret)` issues typed `JwtClaims`
through `jsonwebtoken`. Supply a server-owned subject, expiry in UNIX seconds,
roles, permissions, scopes, features and subscription state. The function rejects
empty subjects/keys; applications own expiry policy, secret generation, rotation
and key distribution. `verify_jwt_hs256` checks the signature, strict expiry and optional not-before
time before returning a principal. It rejects empty keys and malformed optional
`nbf`/`iat` claims; it does not add implicit clock tolerance. For asymmetric keys and issuer/audience/algorithm policy,
use `jwt::verify_jwk`, `verify_jwks`, or the optional remote-JWKS adapter with an
explicit `Validation`. Token-provided key URLs never select trusted keys.

The [canonical IAM example](../blueprint-rs/crates/sample-service/examples/canonical_iam.rs)
shows issuance and router authentication with a test-only key. The actual
TypeScript/JOSE fixture verifies Rust-issued claims and both servers accept the
token; JOSE-issued tokens also authenticate against Rust. Declared JWT routes
return 401 for missing credentials and 403 for invalid tokens. Opaque bearer
session rejection remains 401. Native HTTP error bodies retain their structured
JSON envelope; complete TypeScript error-envelope parity is still tracked.
Better Auth JWT formats and provider/cookie/OIDC flows use their own adapter.

The [engineering security review](../plan/RUST-SECURITY-REVIEW.md) records verified
auth/crypto boundaries, fixed findings and the exact remaining dependency notices.
The audit gate scans all three lockfiles and rejects unreviewed findings.

Runtime performance checks cover HTTP, typed validation, generated SDK calls,
Redis, S3 and typed WebSockets. See [benchmark commands and measurement boundaries](../docs/adding-projects/rust-benchmarks.md)
for optimized runs, optional infrastructure and JSON reports.

`module_auth::HmacJwksAuth::with_hmac_keys_for_route` configures key IDs and
secrets for one declared HMAC route's method and path. Its keys replace the
router's default key policy only on that route. Empty policies, secrets and key
IDs, duplicate IDs, and non-HMAC contracts are rejected. Keys remain server-side
and are absent from contract artifacts. The generated Better Auth relay uses
this mechanism for instance-ingest and platform-claim keys.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

## Package Policy

- Workspace package metadata lives in the root `Cargo.toml`.
- Optional integrations should be feature-gated at the crate that owns the integration.
- Core contract types should stay independent of any specific HTTP framework.
- `forklaunch-axum` is the first adapter, not the framework core.

## Additional MCP tools

Enable `forklaunch-mcp`'s `rmcp-server` feature and append a `server::ManualTool::typed` binding to `McpServerOptions.manual_tools` before calling `ForkLaunchRouter::into_router_with_mcp(options)` (or `server::mount_http`). Input types implement `forklaunch_validation::FrameworkSchema`: serde parses arguments, garde validates them and schemars describes discovery metadata. Declare matching schemars constraints when a garde rule should appear in the schema. Unknown-field behavior follows the input type's serde configuration.

```rust,ignore
let tool = ManualTool::typed::<LookupInput, serde_json::Value, _, _>(
    "lookup", "Look up application data", |input, headers| async move {
        // Authorize this operation with fresh request headers and trusted state.
        authorize_lookup(&headers, &input)?;
        Ok(serde_json::json!({"result": lookup(input).await?}))
    },
)?;
options.manual_tools.push(tool);
let router = application.into_router_with_mcp(options)?;
```

Transport authentication still gates every request. Manually bound handlers must apply their own operation/tenant authorization; HTTP route guards apply to contract-derived tools. Every call receives fresh headers, and tool arguments cannot replace them. Invalid typed input fails before application work; handler errors use explicitly public `ManualToolError` messages. Timeouts cancel the invocation future and response size limits apply. Duplicate manual/HTTP names fail at startup. Object results expose structured content; scalars and arrays remain JSON text.

## HTTP and MCP listeners

Generated services call `ForkLaunchRouter::serve_with_default_mcp(listener, shutdown)`. MCP shares the HTTP listener by default. Set `MCP_LISTENER=separate` to use a second listener with the TypeScript HTTP-port-plus-2000 fallback, or set `MCP_PORT`/`MCP_HOST` to select a separate listener explicitly. Port `0` supports ephemeral binds. Only MCP is exposed on the second listener; tool calls still invoke authenticated HTTP handlers internally. Both listeners bind before serving, then drain together on shutdown with a 30-second bound. `into_routers_with_mcp(options)` supplies separate routers for custom orchestration.

## Server-sent events

Enable `forklaunch-axum`'s `sse` feature and pass a stream of
`Result<SseFrame<T>, E>` to `sse::respond(stream, SseOptions::default())`.
`T` implements `FrameworkSchema`; its garde rules validate each payload.
Use `respond_with(stream, options, validator)` for whole-envelope validation.
The application supplies the typed model or validator matching its route contract;
the helper does not infer validation rules from arbitrary contract JSON Schema.

The default `SseValidation::EveryEvent` validates every event. Explicit
`FirstEvent` matches the TypeScript emitter's first-event-only schema validation.
The helper awaits one event before returning headers, allowing first-event errors
to become sanitized HTTP 500 responses. Later failures terminate the body.
An empty source returns an empty successful SSE response. No producer task or
unbounded queue is created, and cancellation drops the source even while awaiting
the first or next event.

`max_event_bytes` defaults to 1 MiB and bounds each encoded wire frame in either
validation mode. String data is emitted as text; other serializable data uses
JSON. Multiline text receives an SSE `data:` prefix on each line. IDs containing
CR, LF or NUL are rejected in either mode. These framing safeguards intentionally
remain stricter than TypeScript's raw field interpolation.

`rust_sse_emitter_parity.mjs` executes the actual TypeScript response parser and
emitter with Zod schemas, compares 16 frames, and verifies first-event validation
and empty-source behavior. `rust_sse_runtime.sh` uses this helper in a relocated
generated service and exercises both generated SDKs, stream failures, cancellation,
resource release, and HTTP/MCP startup and shutdown.

## Typed WebSocket connections

Enable `forklaunch-ws` features `transport` and optionally `validation`. `transport::TypedWebSocketServer::bind(address, schemas, options)` owns a TCP listener; `accept_with(callback)` authorizes upgrade headers before accepting a connection. `transport::connect(request, schemas.reversed(), options)` accepts a URL or tungstenite request with caller headers/subprotocols and supports TLS with WebPKI roots. Applications own connection tasks and shutdown.

`SocketSchemas::new(incoming, outgoing)` accepts `PayloadCodec::serde()` or `PayloadCodec::framework()` for serde/garde validation; builders add typed ping, pong and close reason schemas. Messages use raw UTF-8 strings or JSON, matching the TypeScript wrappers. Explicit `PayloadCodec::bytes()` bypasses text/JSON interpretation for native binary protocols. The older tagged `WebSocketEnvelope` is a separate wire format.

Checked contracts can declare raw bytes with `SocketSchemaEntry::binary()` (or
`"binary": true` plus `"shape": {"type":"string","format":"binary"}` in the
portable artifact). Both generated clients preserve those bytes as `Vec<u8>` or
`Uint8Array`, including invalid UTF-8 and JSON-looking data. Byte limits still
apply to messages and ping/pong controls. Each binary direction must have one
entry with the canonical shape; mixed JSON/binary unions, extra assertions and
binary close reasons are rejected. A JSON Schema `format: binary` alone remains
an annotation and does not select the binary codec. AsyncAPI export marks binary
messages `application/octet-stream`; the external AsyncAPI importer accepts
canonical binary schemas with that media type.


`SocketSchemaEntry::json(shape)` (portable metadata `jsonEncoding: true`) selects
explicit JSON wire encoding. Generated clients quote strings, preserve `null`,
and reject non-JSON input and invalid UTF-8. Native runtime codecs opt in with
`PayloadCodec::json_schema(&shape)?.with_json_encoding()` or
`PayloadCodec::serde().with_json_encoding()`. Limits count actual encoded bytes,
including string quotes and escapes. Mixed raw/JSON alternatives in one direction
and simultaneous binary/JSON flags are rejected. External AsyncAPI JSON messages
select this mode automatically; native entries created with `new` retain the
TypeScript-compatible raw-string behavior. Exported explicit JSON messages always
carry `application/json` media.

`SocketOptions` bounds frame/message/write buffers and handshake/write deadlines. Invalid incoming payloads close with 1008 before application delivery; invalid outgoing payloads fail locally. Automatic pong replies retain exact bytes and satisfy the outgoing pong schema. `recv()` returns typed message/control events; after `close(code, reason)`, continue receiving to finish the handshake. Dropping a connection releases it; cancelling a pending receive preserves subsequent messages. With `forklaunch-axum` feature `websocket`, `websocket::upgrade(upgrade, schemas, options, handler)` adapts an authorized Axum `WebSocketUpgrade` into `AxumTypedSocket`, using the same validation/control runtime. Compose that route with the HTTP/MCP router on one listener. Select subprotocols or an upgrade-failure callback on the extractor before passing it in; `transport::connect_with_response` retains the negotiated client response. Apply authorization in the HTTP handler before upgrading and propagate application shutdown to active connection tasks. HTTP server handshake timeouts apply before upgrade; socket write deadlines apply afterward. Cancelling an in-progress write releases a partial connection. `SocketContract`/`SocketSchemaEntry` retain raw payload schemas, channel/operation enrichment, controls and context/user-data metadata. `SocketSchemaEntry::framework::<T>()` derives framework schema metadata. `forklaunch_asyncapi::generate_raw_socket_asyncapi_json` exports raw payloads using TypeScript channel fan-out and singular/plural precedence; `_with_options` accepts description, ID and content type. Errors are documented without operations; ping/pong/close controls are omitted, matching TypeScript. JSON-schema document-local pointers relocate to each embedded payload while literal `$ref` data remains intact. Conflicting definitions, invalid addresses, unresolved/external references and unsupported resource scopes fail explicitly. Bundled IDs, static anchors and declared dynamic/recursive scopes are resolved before payload relocation. Schema recursion is bounded to 64 levels, and boolean schemas use AsyncAPI's object form. This raw exporter is separate from tagged-envelope exports. CLI socket-contract generation below uses this checked exporter.

Enable the optional `json-schema` feature for `PayloadCodec::<T>::json_schema(&schema)`. It compiles constraints once, checks incoming JSON before serde can discard unknown fields, and checks serialized outgoing values before raw string encoding. Missing dialect declarations use Draft 7; declared 2020-12 schemas, recursive pointers and in-document resource IDs are supported. File and network retrieval always fail, even when another dependency enables resolver features. Schema compilation bounds depth, nodes, bytes and regex execution. Email uses the AJV full-format profile (dotted domains), and UUID assertions apply in Draft 7 too. Compiled codecs accept safe integral decimal/exponent spellings for schema checks and typed integers, while float/JSON-value targets preserve negative zero. Strings and fractional values are not coerced; larger numbers retain Rust representation/range limits. Decimal `multipleOf` checks are exact; a TypeScript AJV consumer needs the equivalent exact decimal keyword rather than floating-point tolerance. Valid JSON beyond serde's numeric/depth representation limits is rejected instead of becoming plain text. The raw string ambiguity remains: sending a schema-valid string `"42"` emits `42`, which a receiving JSON-first decoder interprets as a number. This codec does not add binary schemas or change the checked exporter's reference restrictions.

The actual TypeScript wrappers also validate automatic control replies before typed delivery. A rejected reply reports one error and closes with 1008, releasing an unresponsive peer after five seconds. Manual invalid control payloads remain local failures, and `autoPong: false` is preserved. Typed close listeners report abnormal 1006 disconnects as transport errors rather than validating an empty reason against the close schema. `rust_ws_runtime.mjs` exercises these paths against real sockets in CI.

`rust_ws_jsonschema_parity.mjs` checks 4,104 encode/decode cases against actual TypeScript WebSocket helpers with AJV and an exact decimal keyword. The separate Zod fixture checks 168 cases, including JSON representation-limit rejection.

`forklaunch_sdk_codegen::websocket::SocketSdkPlan::from_contract(&contract)` prepares checked client directions, protocol controls and recursive message unions. `forklaunch_sdk_rust::generate_rust_websocket_sdk_source(&plan, &RustSdkOptions::new("chat-sdk"))` emits a native client with typed `send`, `recv`, `ping`, `pong` and `close`. `connect(request, options)` returns the client and HTTP upgrade response; caller requests retain authorization headers and subprotocols. The generated source needs `forklaunch-ws` features `transport,json-schema`, serde `derive`, `serde_json`, tokio `net` and tokio-tungstenite 0.30. `SOCKET_PATH` exposes the declared path; callers supply the complete request URL. `schemas()` prepares reusable validated codecs, and socket accessors allow composition with the native runtime. Error/context/user-data metadata describe framework events, rather than extra wire messages. Unions retain each standalone schema's recursive references and require a common dialect; bundled resource IDs and declared dynamic/recursive references are resolved independently before union relocation. Messages may reuse resource IDs without sharing bindings. CLI socket-contract generation below reuses this generator.

`forklaunch_sdk_typescript::generate_typescript_websocket_sdk_source(&plan, &TypeScriptWebSocketSdkOptions::new("ChatClient"))` emits a Node client using ws 8.22, AJV 8, ajv-draft-04 1 and ajv-formats 3, with Node/ws type declarations for TypeScript consumers. `await ChatClient.connect(url, options)` retains upgrade headers and negotiated protocol in `response`/`protocol`. `send`, `ping` and `pong` return promises; `close(code, reason)` starts a handshake, and `recv()` yields typed `{ type, value }` message/control events or `{ type: 'close', code, reason }`. After a terminal error is reported, receiving ends with `null`. Consume close events to finish a graceful handshake, or call `terminate()` to release the connection immediately. Breaking an async iterator releases it too.

The Node client bounds message bytes (1 MiB), queued events (128), queued wire payload bytes (4 MiB), handshake time (10 seconds), write/automatic-pong time (5 seconds) and close time (5 seconds); options override these positive limits within Node's timer range. Receive abort/timeouts preserve the next event, while cancelling a partial write releases the connection. One explicit receive and one explicit write may be pending at a time. The raw JSON profile uses exact decimal multiples, own-property checks and reference-safe normalization for AJV's excluded `__proto__` properties. Literal schema data and existing pattern/reference targets remain unchanged. These generated clients use JSON/raw UTF-8 strings; explicit raw binary SDK metadata is still tracked separately.

Generated services also export sockets registered in `contract_registry()`. With `forklaunch-contract/json-schema` enabled (already enabled through generated OpenAPI dependencies), register a `forklaunch_ws::SocketContract`:

```rust
registry.register_socket("chat", &chat_contract).expect("register chat socket");
```

`forklaunch openapi export` writes registered socket metadata into the service contract and `.forklaunch/socket-contracts/<service>/socket-contract.json`, using the service name/version. It honors project metadata `socketContractArtifactPath`. A content ownership record lets later exports remove an unchanged generated artifact when its path moves or the last socket is removed; manual and edited artifacts are retained. Run export after Rust registry changes, then generate/watch the artifacts. Runtime upgrade handlers are composed by the application.

`SocketContractArtifact::new(title, version).with_socket("chat", contract)` produces a versioned, portable collection of named socket contracts. Serialize it to `.forklaunch/socket-contracts/<service>/socket-contract.json`, then run:

```sh
forklaunch sdk generate --from socket-contract --target all --service accounts
forklaunch sdk generate --from socket-contract --target all --check
forklaunch sdk watch --from socket-contract --target all
```

The CLI runs the checked generators from the application's bundled Rust framework, with a five-minute compiler deadline and bounded input/output. It emits `.forklaunch/asyncapi/<service>/asyncapi.json`, Node packages under `sdks/typescript-websocket/<service>` and Rust crates under `sdks/rust/<service>-websocket-sdk`. Each named socket has a `socket_<name>` module and `SocketClient`; root TypeScript exports use namespaces. Socket keys contain lowercase ASCII letters, digits or underscores. Cargo and PNPM workspace registration is automatic, and existing TypeScript compiler configuration is retained. Optional project metadata `socketContractArtifactPath` and `asyncapiPath` select custom paths inside the application; watch follows configured contract paths and manifest changes. After startup it reports invalid edits, retains the last generated outputs and retries when artifacts change; repeated manifest-read errors are reported once. All requested services are checked before any SDK files are written. `rust_ws_cli_runtime.sh` validates relocated generation, drift checks, multi-service failure staging, path containment, custom-path watching, both live clients, the official AsyncAPI parser and generated HTTP/MCP startup. Registry export lifecycle, custom paths and ownership are exercised through the relocated CLI fixture and archive installer. Regeneration prunes unchanged obsolete modules. Exported empty registries retire owned SDK files/AsyncAPI/Cargo members; transient missing artifacts and edited/untracked files are preserved, and live watch restores packages when sockets return. Explicit service deletion also cleans owned socket outputs. Shared pnpm package globs remain for retained user packages. Service rename updates socket artifact/package names and Cargo membership, preserves custom compiler options and retains edited/untracked old files. The relocated fixture compiles both clients and validates HTTP/MCP startup after default/custom-path renames. External AsyncAPI documents use the explicit server-view import profile below.

`rust_ws_typescript_sdk_runtime.mjs` tests both module formats and declarations, normal/recursive/empty/Draft-4/Draft-6 clients, actual TypeScript/native peers, all 4,104 codec cases, protected keys and scoped references, malformed-peer 1008 rejection, queue limits, iterator cleanup, and stalled/cancelled handshakes/writes/pongs/closes. Generated TypeScript clients support Draft 4, Draft 6, Draft 7, 2019-09 and 2020-12. The official Draft 6 meta-schema is embedded in emitted source for strict ESM/CommonJS consumers; no runtime meta-schema imports or retrieval are required. Draft 4 uses `ajv-draft-04` with newer validation keywords disabled; shared model generation also ignores those keywords, preserving declared field types. Legacy Draft 4 `id` resource scopes use the same bounded offline resolver as modern IDs. Ignored older-dialect annotations retain their exported literal data. Generated validators clean inactive resource IDs on a private copy because AJV discovers them independently of its dialect vocabulary; explicitly referenced schemas and literal IDs in active assertions remain intact. Shared SDK models and OpenAPI reference objects decode URI fragments once, including Unicode, escaped pointer tokens and literal percent names; equivalent recursive edges reuse one model. Shared relocation emits URI-safe fragments for nested union schemas and metadata exports. The archive installer also generates and compiles both languages' WebSocket consumers using its relocated bundled sources and pinned npm dependencies.

`rust_ws_sdk_runtime.mjs` compiles normal, recursive and empty native clients with warnings denied, exercises the normal client against the actual TypeScript server, and verifies malformed peer payloads close with 1008 before delivery.

`rust_ws_metadata_parity.mjs` compares 34 documents with the actual TypeScript generator and validates all against the official AsyncAPI JSON Schema. Parser 3.6.3 also accepts 63 documents; eleven cases expose its percent/prototype map-key and literal `$ref` limitations, so those retain direct specification-schema and pointer/AJV checks. Recursive structural schemas and explicitly referenced legacy annotation storage additionally pass the parser without changing unreferenced literal data. Schema traversal follows reachable local pointers exactly once, decodes percent escapes once, and enforces 64-step/100,000-location bounds. Checked exports now bundle schemas referenced from active const/enum/default/example data into collision-checked definitions, preserving the original literal values and recursive references. Copying and final output share 64-level, 100,000-node and 32 MiB bounds; repeated bundling is stable. The same relocation helper protects OpenAPI/MCP schemas and generated socket unions, preserving embedded resource bases. Checked unscoped socket export bundles references to entire schema dictionaries in separate schema/literal interpretation contexts; resource-preserving relocation retains its conservative alias restrictions. Declared Draft 4/6/2019-09/2020-12 payloads use AsyncAPI Multi Format Schema Objects; the parser accepts these custom formats but leaves their schema validation to the independently checked native/codecs. This preserves older boolean exclusive bounds and avoids imposing Draft 7 semantics.

`forklaunch_contract::bundle_document_schema(&document, "/components/schemas/Message")` extracts a standalone JSON Schema from a containing document. It bundles transitive local references, including recursive and literal-data targets, into collision-checked definitions. It preserves literal values and rejects missing/external references and active resource IDs/dynamic scope under shared copy/output limits. Dictionary references use separate interpretation contexts: a property named `const` can remain a schema in the original dictionary and literal data in a referenced copy. Recursive edges are rebased without rewriting annotation data. `forklaunch_sdk_codegen::bundle_document_schema_resources` adds offline resource IDs, anchors and dynamic/recursive roots to document extraction; resource-free documents retain the unscoped path. Ambiguous resource-aware dictionary aliases still fail explicitly. The checked AsyncAPI importer uses this shared resolver with explicit operation roles.

Import an AsyncAPI **3.0.0 WebSocket server** document with:

```sh
forklaunch sdk generate --from asyncapi --target all --service realtime
forklaunch sdk watch --from asyncapi --target all --service realtime
```

The input defaults to `.forklaunch/asyncapi/<service>/asyncapi.json`; project metadata `asyncapiArtifactPath` selects another contained path. Generation/check/watch preserve the input document. SDK ownership records retain their source through service rename; standard input paths move with the service without changing document bytes, while custom paths remain custom. Regeneration removes obsolete unchanged modules and preserves edited files and compiler settings. All services validate before outputs are written, and outputs cannot overwrite input artifacts. Importing a formerly generated AsyncAPI file releases its export ownership without deleting it.

The native API is `forklaunch_asyncapi::import_server_websocket_asyncapi` with the `import` feature. Server `receive` operations become client outgoing messages, and server `send` operations become client incoming messages. Channels need unique absolute paths, explicit operation message subsets, JSON or `application/octet-stream` media and ws/wss servers; GET bindings without upgrade parameter schemas are supported. An offline, licensed official specification schema validates the input before bounded local-reference extraction. Declared payload constraints are preserved, multiple messages retain `oneOf`, and undeclared directions reject payloads. Channel message schemas must have provably disjoint root types or required string discriminators. Bundled component/payload resource IDs, URI aliases, static/dynamic anchors and recursive roots are resolved offline. Different messages may declare different supported dialects, but each message must retain one dialect across its reachable resource graph. Missing external resources, duplicate IDs, mixed runtime dialects, headers/traits/replies/security, parameterized paths and other protocols fail checked import. Binary messages require the exact payload `{ "type": "string", "format": "binary" }`, inline or through bounded local references, and `application/octet-stream` on the message or document default. Each binary direction selects exactly one message. Send and receive may use different wire types; mixed JSON/binary alternatives within one direction and extra byte-schema assertions fail explicitly. The generated clients use `Vec<u8>` and `Uint8Array`, preserving invalid UTF-8 and JSON-looking byte sequences. Binary schema annotations alone never select a binary codec for JSON media. Broader event projects and profiles remain open. The relocated `rust_asyncapi_import_runtime.sh` fixture compiles both SDKs, runs live peers, exercises watch recovery and rename, and validates generated HTTP/MCP listener startup and shutdown.


Multi Format JSON payloads support Draft 4, Draft 6, Draft 7, 2019-09 and 2020-12.
Set `schemaFormat` to `application/schema+json;version=<dialect>` and include the
matching explicit `$schema` URI inside `schema`. For example, Draft 4 uses
`application/schema+json;version=draft-04` and
`http://json-schema.org/draft-04/schema#`. The importer preserves dialect-specific
constraints and document-local references; message unions retain their root
dialect. Every alternative in one direction must use the same dialect. Missing
or mismatched declarations, unknown formats and nested mixed dialects fail checked
import. Draft 4 message discrimination requires `enum` tags because its `const`
keyword is only an annotation. `rust_asyncapi_dialects_runtime.mjs` compiles both
SDKs for all five dialects, tests live roundtrips and rejects invalid numeric bounds.

Checked SDK schemas, raw socket AsyncAPI exports and imported AsyncAPI payloads require literal `/` JSON-pointer separators; use `~1` for a slash within a name. Percent-encoded names, escaped tokens and literal percent names remain supported. Encoded separators fail before artifact writes, matching the checked validator profile. Generic OpenAPI reference-object resolution and resource-preserving relocation retain URI decoding independently. Native raw JSON Schema codecs and actual AJV still differ for encoded interior separators outside this checked profile; broader resolver parity remains tracked.

See the [Rust/TypeScript compatibility baseline](../docs/adding-projects/rust-compatibility.md) for source package versions, artifact formats and implementation choices.

## Reviewing generated output changes

The TypeScript SDK crate's `snapshots::openapi_and_both_http_sdks_match_reviewed_snapshots`
test compares a shared catalog contract against committed OpenAPI, Rust HTTP SDK
and TypeScript HTTP SDK output. It covers path/query/header parameters, optional
nullable fields, typed success/error responses, response headers and empty 204s.
The normal workspace test job runs this check in CI. These are native output
regressions; the cross-language conformance fixtures remain separate.

For an intentional generator change, run from this workspace:

```bash
FORKLAUNCH_UPDATE_SNAPSHOTS=1 cargo test -p forklaunch-sdk-typescript snapshots
cargo test -p forklaunch-sdk-typescript snapshots
```

Review the source and snapshot diffs together before committing. CI refuses the
update flag. OpenAPI is compared as sorted, pretty-printed JSON; SDK source is
compared with trailing line whitespace normalized. No timestamps or absolute build paths are included.

### Optional TanStack Query helpers

Generated HTTP TypeScript SDKs export dependency-free `forkLaunchQueryOptions`
and `forkLaunchMutationOptions`. Import them when using TanStack Query v5; the
ordinary SDK keeps its existing call shape and status-union behavior.

```typescript
import { queryOptions, useQuery, useMutation } from '@tanstack/react-query';
import { createForkLaunchSdk, forkLaunchQueryOptions, forkLaunchMutationOptions } from './sdks/typescript/src/index';

const sdk = createForkLaunchSdk({ baseUrl: 'https://api.example.com' });
const users = useQuery(queryOptions(forkLaunchQueryOptions(
  ['tenant', tenantId, 'accounts', 'users'],
  sdk.accounts.users.listUsers,
  {},
)));
const remove = useMutation(forkLaunchMutationOptions(
  ['tenant', tenantId, 'accounts', 'deleteUser'],
  sdk.accounts.users.deleteUser,
));
```

Use your generated operation names. The helper retains the complete successful
HTTP envelope (`status`, `code`, `response`, content type and headers), narrowing
away non-2xx variants. Non-2xx responses throw `ForkLaunchSdkHttpError`, whose
`result` retains the returned envelope; transport errors propagate unchanged.
Request parameters and mutation variables retain the generated schema types.
Queries propagate TanStack's cancellation signal and combine it with a caller
signal, removing listeners after settlement. Mutations retain caller request
options and do not invent automatic cancellation or invalidation policies.

Keys are explicit: include the operation, resource arguments, endpoint identity
and tenant/user cache scope appropriate to your application. Do not put bearer
tokens or other secrets into keys. Clear scoped cached data when a session ends.
Spread TanStack options such as `staleTime`, `retry` or `select` at the call site;
wrap the result with TanStack's `queryOptions` when you need its tagged-key type
inference for `getQueryData`. Streaming iterators should be consumed explicitly
rather than cached as reusable results.

The fixture `cli/tests/rust_tanstack_query_parity.mjs` checks actual Query Core and
React Query **5.104.2** types, cache reuse/invalidation and scope separation,
HTTP failures and retries, mutations, network failures, and both sources of
cancellation against generated fetch clients. It follows the upstream
[query-options](https://tanstack.com/query/latest/docs/framework/react/guides/query-options)
and [TypeScript integration](https://tanstack.com/query/latest/docs/framework/react/typescript)
interfaces. Configure `FORKLAUNCH_TEST_TANSTACK_DEPENDENCIES` to an isolated npm
prefix containing the pinned dependencies listed in the Rust parity workflow.

### Literal JSON object keys

The framework workspace and generated applications select a pinned, vendored
Serde JSON 1.0.151 patch. Literal object keys named `$serde_json::private::Number`
or `$serde_json::private::RawValue` remain object data even when dependency
features enable exact numbers and raw JSON. Axum, MCP, typed request fields and
encrypted JSON record codecs all use that same parser. Real exact numbers and
RawValue conversions retain their existing behavior.

Generated roots contain a `[patch.crates-io]` entry pointing at
`.forklaunch/framework-rs/crates/vendor/serde_json`. Generation/sync adds a missing
entry in existing applications and preserves explicit user patches. Standalone
consumers outside these roots must apply an equivalent workspace patch; Cargo
does not inherit patches from dependencies. The checked package versions remain
pinned so later registry releases cannot silently bypass the patch.

The [JSON boundary audit](../plan/RUST-JSON-BOUNDARY-AUDIT.md) records coverage and
remaining limits, including unpaired surrogates, bounded nesting, and independent
JavaScript/ORM normalization semantics. This does not claim upstream adoption or
registry publication of the patched parser.


### Typed multipart requests

Enable `forklaunch-axum`'s `multipart` feature and use
`multipart::Multipart<Upload>` as the last handler extractor. `Upload` implements
`FrameworkSchema` (Serde, Schemars and garde); parsing and validation finish before
the handler runs. Text parts map to strings, file parts to `Vec<u8>`. Octet-stream
parts without filenames are files too. Repeated names use the last parsed part.
Mixed text/file parts sharing a name are not a portable cross-language contract:
Busboy assigns files on asynchronous stream completion. Use distinct field names.
Filenames are ignored and never used as filesystem paths. Numeric/composite text
coercion requires an explicit Serde model rule.

`Extension(MultipartLimits { parts: 128, field_bytes: 1024 * 1024,
total_bytes: 2 * 1024 * 1024 })` supplies the defaults; customize them per router.
Every part counts, including overwritten values. These are additional limits:
Axum's `DefaultBodyLimit` and `RouterConfig` body limits still apply. Invalid
models/forms return 400, extractor size limits return 413, and invalid zero
limits return 500. Error text omits uploaded values. The framework router's
earlier total-body limit retains its existing framework validation response.

Unlike Busboy's permissive text decoder, this extractor rejects invalid UTF-8;
it does not implement legacy charset conversion. It preserves file bytes,
including empty files, and does not JSON-parse text fields implicitly.
`cli/tests/rust_multipart_extract_parity.mjs` compares seven cases against the
actual TypeScript discriminator/content-parser source with Busboy, including
repeated fields/files, Unicode, binary/empty files and filename-less octet streams.
Multipart SDK response decoding is described below; broader MCP conformance
remains tracked independently.


### Multipart SDK responses

Both native and CLI generators decode declared `multipart/form-data` responses.
TypeScript returns `FormData`; Rust returns `MultipartResponse { parts }`, an
ordered list of names, optional filenames/content types and byte vectors. Repeated
names, binary files and empty files are retained. No part is written to disk.
Multipart/mixed is rejected during generation. The transport result is not an
automatic JSON DTO conversion or validation of individual part schemas.

Responses are buffered with an **8 MiB** default wire-byte limit and **1,024**
parts. TypeScript config `maxMultipartResponseBytes` and Rust client builder
`with_multipart_response_limit(bytes)` customize the positive byte limit. Wrong
media, missing/invalid boundaries, truncation and limit violations reject the
call; early transport failures release the response. TypeScript supports native
fetch and injected cross-fetch byte streams and retains request abort signals.

Rust generated crates add `multer = "3.1"` and `futures-util = "0.3"` only when
responses require them. Consumers of standalone native-generated source must
provide those dependencies alongside reqwest, Serde and Serde JSON.

`cli/tests/rust_http_runtime.sh` compiles both artifact formats and both native
generators, runs multipart cases against Rust and TypeScript servers and checks
HTTP/MCP startup and graceful shutdown. The TypeScript native runtime uses the
platform FormData parser; Rust uses multer.

### Optional browser sessions

Enable `forklaunch-axum/cookie-session` and configure
`RouterConfig.cookie_sessions` with `CookieSessions::new(cookie_name,
"https://your-app.example", store, resolver)`. Declare protected routes with
`AuthConfig::cookie(cookie_name)`. OpenAPI exposes each named cookie as its own
cookie API-key security scheme. Without the feature/configuration these routes
fail closed. Bearer/JWT routes still require their own verifier.

The adapter uses tower-sessions 0.15, secure HttpOnly SameSite=Strict cookies,
path `/`, and a 30-minute expiry refreshed by session writes. Read-only requests do not
extend the lifetime. Supply a shared durable `SessionStore`
in production; MemoryStore is for tests/development. The async resolver receives
the stored subject and must load current authorization and reject revoked or
disabled identities. No roles or provider credentials are copied into the cookie.
Generic session-map updates are not atomic replay or global-revocation storage.

Use the `CookieSession` extractor on login, logout and CSRF bootstrap handlers.
`csrf_token()` issues a random synchronizer token; serve it with `Cache-Control:
no-store` and keep it in the trusted browser context. For login/logout and other
unsafe requests, browsers must send that token in `x-csrf-token`, the session
cookie, and the exact configured HTTPS `Origin`. Missing, duplicate and foreign
origins/tokens fail. Verify credentials before calling `login(subject)`; login
rotates both the session ID and token and returns the replacement token.
`logout()` flushes the session. Both reject safe methods. Plain Axum handlers
can use `policy.wrap(router)` with the same policy/store as the framework router.
Configure credentialed CORS only for trusted origins when needed.

MCP cookie-authenticated tools forward the transport cookie, Origin and CSRF
headers to the same HTTP authentication path; tool arguments cannot replace
these headers. MCP transport authentication and Host/Origin policy remain
independent. This adapter does not replace Better Auth's compatibility cookie
format or implement OAuth/OIDC login providers.

HTTP SDK schema imports support document-local static `$anchor` references in
modern JSON Schema, including percent-encoded names, recursive models and
anchors in OpenAPI component schemas. Pointer and anchor references to the same
schema reuse a model. Indexing follows schema positions under shared depth,
node and byte limits; examples/defaults remain literal values. Duplicate,
invalid, missing and foreign-resource anchors fail before generation. Older
Draft 4/6/7 schemas do not gain modern `$anchor` semantics. Resource `$id`,
external retrieval and dynamic references remain unsupported in SDK imports;
checked AsyncAPI payload imports retain their separate conservative profile.

`cli/tests/rust_mcp_transport_parity.mjs` runs 32 calls through the real pinned
FastMCP server, repository Zod validator/generator and official MCP client, then
compares live HTTP body bytes with the native MCP encoder. Set
`FORKLAUNCH_TEST_CANONICAL_DEPENDENCIES` to the installed canonical-iam fixture
prefix. The MCP link uses the official in-memory transport; forwarded HTTP is a
loopback TCP server. This complements native HTTP/MCP listener/startup tests and
the lower-level exact multipart-framing fixture.


### Bundled schema resource IDs

The shared SDK compiler resolves `$id` resources already bundled in a schema
or OpenAPI document. Relative IDs and references resolve against their lexical
base; pointer fragments address the selected resource, and `$anchor` names are
scoped to that resource. Legacy Draft-4/6/7 fragment-only IDs (`#Node`) also
define scoped anchors, including recursive aliases; IDs beside legacy `$ref`
retain AJV-compatible scope in both the compiler and native codecs. Legacy IDs
combining a URI/path and fragment (`nodes#Node`) resolve as exact bundled
aliases, including nested fragment-only aliases. Such an alias does not create
a bare `nodes` resource or an implicit JSON-pointer base. The compatibility
profile accepts legacy pointer-shaped IDs and digit/underscore names. Exact
registered IDs take precedence over document pointers. URI normalization decodes
unreserved characters and normalizes escape case while keeping encoded
delimiters distinct (`#%2Falias` differs from `#/alias`).
Draft-4 `id` and embedded resources using different
schema drafts retain their own keyword rules; legacy `const`/conditional
annotations do not become modern constraints. Recursive URI/pointer aliases
retain one model identity. Explicit 2020-12 resources support `$dynamicAnchor`
and `$dynamicRef`: outermost dynamic bindings specialize recursive models, while
ordinary `$ref` stays static. Dynamic references to plain anchors or pointers
retain static resolution. Missing dialect declarations do not enable dynamic
keywords. Explicit 2019-09 resources also support root `$recursiveAnchor`
booleans and `$recursiveRef: "#"`, preserving resource-root fallback when no
recursive anchor applies. Relative root IDs use the implementation's retrieval base
`https://forklaunch.invalid/__document__`.
No network fetch occurs during generation. Literal defaults/examples are not
interpreted as resource declarations. The same depth/node/byte budgets apply to
the resource index and model graph.

This profile rejects duplicate IDs, missing resources, malformed IDs, nonempty
modern ID fragments, non-root recursive anchors, non-`#` recursive references,
and resource-aware references into literal data/schema dictionaries. WebSocket
SDK unions and raw AsyncAPI export resolve each standalone resource graph before
relocation; resource-free schemas retain their original annotation/pointer layout.
`forklaunch_sdk_codegen::bundle_schema_resources` exposes this bounded operation.
Runtime relocation requires one dialect within each message; mixed embedded
dialects remain a model-import capability until runtime constraints can be
translated without changing their meaning. An explicit rejection is preferable to
silently generating a model for the wrong schema resource. Run
`cli/tests/rust_schema_resources_runtime.sh` for live recursive Rust/TypeScript
SDK checks from contract and OpenAPI artifacts, including HTTP/MCP startup.
