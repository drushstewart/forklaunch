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

`SocketOptions` bounds frame/message/write buffers and handshake/write deadlines. Invalid incoming payloads close with 1008 before application delivery; invalid outgoing payloads fail locally. Automatic pong replies retain exact bytes and satisfy the outgoing pong schema. `recv()` returns typed message/control events; after `close(code, reason)`, continue receiving to finish the handshake. Dropping a connection releases it; cancelling a pending receive preserves subsequent messages. With `forklaunch-axum` feature `websocket`, `websocket::upgrade(upgrade, schemas, options, handler)` adapts an authorized Axum `WebSocketUpgrade` into `AxumTypedSocket`, using the same validation/control runtime. Compose that route with the HTTP/MCP router on one listener. Select subprotocols or an upgrade-failure callback on the extractor before passing it in; `transport::connect_with_response` retains the negotiated client response. Apply authorization in the HTTP handler before upgrading and propagate application shutdown to active connection tasks. HTTP server handshake timeouts apply before upgrade; socket write deadlines apply afterward. Cancelling an in-progress write releases a partial connection. `SocketContract`/`SocketSchemaEntry` retain raw payload schemas, channel/operation enrichment, controls and context/user-data metadata. `SocketSchemaEntry::framework::<T>()` derives framework schema metadata. `forklaunch_asyncapi::generate_raw_socket_asyncapi_json` exports raw payloads using TypeScript channel fan-out and singular/plural precedence; `_with_options` accepts description, ID and content type. Errors are documented without operations; ping/pong/close controls are omitted, matching TypeScript. JSON-schema document-local pointers relocate to each embedded payload while literal `$ref` data remains intact. Conflicting definitions, invalid addresses, unresolved/external references and resource IDs/anchors fail explicitly. Schema recursion is bounded to 64 levels, and boolean schemas use AsyncAPI's object form. This raw exporter is separate from tagged-envelope exports. Generated clients and CLI artifact integration remain tracked in the implementation plan.

`rust_ws_metadata_parity.mjs` compares 28 documents with the actual TypeScript generator and validates all against the official AsyncAPI JSON Schema. Parser 3.6.3 also validates 49 documents; nine cases expose its percent/prototype map-key and literal `$ref` limitations, so those retain direct specification-schema and pointer/AJV checks. Recursive structural schemas additionally pass the parser without changing the original literal assertions.
