# framework-rs

Rust-native ForkLaunch framework crates.

This workspace implements contract registration, validation primitives, an Axum HTTP adapter, OpenAPI and AsyncAPI generation, MCP tool generation and an optional Streamable HTTP server, SDK code generation, and infrastructure interfaces. Optional runtime integrations are feature-gated: PostgreSQL tenant transactions, TypeScript-compatible field encryption, Redis storage, and remote JWKS verification. Axum supports verified sessions, RBAC, and OpenTelemetry request instruments. Production integration and cross-language parity work remains tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md).

The CLI can generate runnable Rust backends with bundled framework source. See [Rust backend generation](../docs/adding-projects/rust.md) for setup, contract exports, SDKs, and current boundaries.

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

## Typed WebSocket connections

Enable `forklaunch-ws` features `transport` and optionally `validation`. `transport::TypedWebSocketServer::bind(address, schemas, options)` owns a TCP listener; `accept_with(callback)` authorizes upgrade headers before accepting a connection. `transport::connect(request, schemas.reversed(), options)` accepts a URL or tungstenite request with caller headers/subprotocols and supports TLS with WebPKI roots. Applications own connection tasks and shutdown.

`SocketSchemas::new(incoming, outgoing)` accepts `PayloadCodec::serde()` or `PayloadCodec::framework()` for serde/garde validation; builders add typed ping, pong and close reason schemas. Messages use raw UTF-8 strings or JSON, matching the TypeScript wrappers. Explicit `PayloadCodec::bytes()` bypasses text/JSON interpretation for native binary protocols. The older tagged `WebSocketEnvelope` is a separate wire format.

`SocketOptions` bounds frame/message/write buffers and handshake/write deadlines. Invalid incoming payloads close with 1008 before application delivery; invalid outgoing payloads fail locally. Automatic pong replies retain exact bytes and satisfy the outgoing pong schema. `recv()` returns typed message/control events; after `close(code, reason)`, continue receiving to finish the handshake. Dropping a connection releases it; cancelling a pending receive preserves subsequent messages. With `forklaunch-axum` feature `websocket`, `websocket::upgrade(upgrade, schemas, options, handler)` adapts an authorized Axum `WebSocketUpgrade` into `AxumTypedSocket`, using the same validation/control runtime. Compose that route with the HTTP/MCP router on one listener. Select subprotocols or an upgrade-failure callback on the extractor before passing it in; `transport::connect_with_response` retains the negotiated client response. Apply authorization in the HTTP handler before upgrading and propagate application shutdown to active connection tasks. HTTP server handshake timeouts apply before upgrade; socket write deadlines apply afterward. Cancelling an in-progress write releases a partial connection. `SocketContract`/`SocketSchemaEntry` retain raw payload schemas, channel/operation enrichment, controls and context/user-data metadata. `SocketSchemaEntry::framework::<T>()` derives framework schema metadata. `forklaunch_asyncapi::generate_raw_socket_asyncapi_json` exports raw payloads using TypeScript channel fan-out and singular/plural precedence; `_with_options` accepts description, ID and content type. Errors are documented without operations; ping/pong/close controls are omitted, matching TypeScript. JSON-schema document-local pointers relocate to each embedded payload while literal `$ref` data remains intact. Conflicting definitions, invalid addresses, unresolved/external references and resource IDs/anchors fail explicitly. Schema recursion is bounded to 64 levels, and boolean schemas use AsyncAPI's object form. This raw exporter is separate from tagged-envelope exports. CLI socket-contract generation below uses this checked exporter.

Enable the optional `json-schema` feature for `PayloadCodec::<T>::json_schema(&schema)`. It compiles constraints once, checks incoming JSON before serde can discard unknown fields, and checks serialized outgoing values before raw string encoding. Missing dialect declarations use Draft 7; declared 2020-12 schemas, recursive pointers and in-document resource IDs are supported. File and network retrieval always fail, even when another dependency enables resolver features. Schema compilation bounds depth, nodes, bytes and regex execution. Email uses the AJV full-format profile (dotted domains), and UUID assertions apply in Draft 7 too. Compiled codecs accept safe integral decimal/exponent spellings for schema checks and typed integers, while float/JSON-value targets preserve negative zero. Strings and fractional values are not coerced; larger numbers retain Rust representation/range limits. Decimal `multipleOf` checks are exact; a TypeScript AJV consumer needs the equivalent exact decimal keyword rather than floating-point tolerance. Valid JSON beyond serde's numeric/depth representation limits is rejected instead of becoming plain text. The raw string ambiguity remains: sending a schema-valid string `"42"` emits `42`, which a receiving JSON-first decoder interprets as a number. This codec does not add binary schemas or change the checked exporter's reference restrictions.

The actual TypeScript wrappers also validate automatic control replies before typed delivery. A rejected reply reports one error and closes with 1008, releasing an unresponsive peer after five seconds. Manual invalid control payloads remain local failures, and `autoPong: false` is preserved. Typed close listeners report abnormal 1006 disconnects as transport errors rather than validating an empty reason against the close schema. `rust_ws_runtime.mjs` exercises these paths against real sockets in CI.

`rust_ws_jsonschema_parity.mjs` checks 3,888 encode/decode cases against actual TypeScript WebSocket helpers with AJV and an exact decimal keyword. The separate Zod fixture checks 168 cases, including JSON representation-limit rejection.

`forklaunch_sdk_codegen::websocket::SocketSdkPlan::from_contract(&contract)` prepares checked client directions, protocol controls and recursive message unions. `forklaunch_sdk_rust::generate_rust_websocket_sdk_source(&plan, &RustSdkOptions::new("chat-sdk"))` emits a native client with typed `send`, `recv`, `ping`, `pong` and `close`. `connect(request, options)` returns the client and HTTP upgrade response; caller requests retain authorization headers and subprotocols. The generated source needs `forklaunch-ws` features `transport,json-schema`, serde `derive`, `serde_json`, tokio `net` and tokio-tungstenite 0.30. `SOCKET_PATH` exposes the declared path; callers supply the complete request URL. `schemas()` prepares reusable validated codecs, and socket accessors allow composition with the native runtime. Error/context/user-data metadata describe framework events, rather than extra wire messages. Unions retain each standalone schema's recursive references and require a common dialect; resource IDs/dynamic references remain unsupported by the model compiler. CLI socket-contract generation below reuses this generator.

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

The CLI runs the checked generators from the application's bundled Rust framework, with a five-minute compiler deadline and bounded input/output. It emits `.forklaunch/asyncapi/<service>/asyncapi.json`, Node packages under `sdks/typescript-websocket/<service>` and Rust crates under `sdks/rust/<service>-websocket-sdk`. Each named socket has a `socket_<name>` module and `SocketClient`; root TypeScript exports use namespaces. Socket keys contain lowercase ASCII letters, digits or underscores. Cargo and PNPM workspace registration is automatic, and existing TypeScript compiler configuration is retained. Optional project metadata `socketContractArtifactPath` and `asyncapiPath` select custom paths inside the application; watch follows configured contract paths and manifest changes. After startup it reports invalid edits, retains the last generated outputs and retries when artifacts change; repeated manifest-read errors are reported once. All requested services are checked before any SDK files are written. `rust_ws_cli_runtime.sh` validates relocated generation, drift checks, multi-service failure staging, path containment, custom-path watching, both live clients, the official AsyncAPI parser and generated HTTP/MCP startup. Registry export lifecycle, custom paths and ownership are exercised through the relocated CLI fixture and archive installer. Importing external AsyncAPI documents remains a separate parity task.

`rust_ws_typescript_sdk_runtime.mjs` tests both module formats and declarations, normal/recursive/empty/Draft-4/Draft-6 clients, actual TypeScript/native peers, all 3,888 codec cases, protected keys and scoped references, malformed-peer 1008 rejection, queue limits, iterator cleanup, and stalled/cancelled handshakes/writes/pongs/closes. Generated TypeScript clients support Draft 4, Draft 6, Draft 7, 2019-09 and 2020-12. The official Draft 6 meta-schema is embedded in emitted source for strict ESM/CommonJS consumers; no runtime meta-schema imports or retrieval are required. Draft 4 uses `ajv-draft-04` with newer validation keywords disabled; shared model generation also ignores those keywords, preserving declared field types. Legacy Draft 4 `id` resource scopes fail before generation, alongside the existing resource-ID restrictions. The archive installer also generates and compiles both languages' WebSocket consumers using its relocated bundled sources and pinned npm dependencies.

`rust_ws_sdk_runtime.mjs` compiles normal, recursive and empty native clients with warnings denied, exercises the normal client against the actual TypeScript server, and verifies malformed peer payloads close with 1008 before delivery.

`rust_ws_metadata_parity.mjs` compares 32 documents with the actual TypeScript generator and validates all against the official AsyncAPI JSON Schema. Parser 3.6.3 also accepts 57 documents; nine cases expose its percent/prototype map-key and literal `$ref` limitations, so those retain direct specification-schema and pointer/AJV checks. Recursive structural schemas additionally pass the parser without changing the original literal assertions. Declared Draft 4/6/2019-09/2020-12 payloads use AsyncAPI Multi Format Schema Objects; the parser accepts these custom formats but leaves their schema validation to the independently checked native/codecs. This preserves older boolean exclusive bounds and avoids imposing Draft 7 semantics.
