---
'@forklaunch/core': minor
---

Contract, documentation and validation fixes:

- New exports from `@forklaunch/core/http`: `bindRuntimeAuth` (binds JWT or HMAC credentials to a portable auth policy and rejects bindings that try to override the policy) and `registerDelegatedSurface` (registers provider-owned routes, such as Better Auth's, and keeps them out of generated endpoint SDKs).
- OpenAPI: path parameters are marked `required`; security requirements name the scheme that is declared (the auth header name, or `basic`) with an empty scope list; `undefined`/`void` responses have no `content`; a request body is optional when its schema is optional (through the validator's new `openapiRequestBody`).
- Response validation: in `error` mode, a status code the contract does not declare is a 500 (`Undocumented response status`) instead of being validated against a missing schema; `warning` mode still sends and logs it. Framework diagnostics are no longer re-validated against the failing route.
- Request validation errors (400) are sent as `text/plain`, matching their body.
- MCP tools: versioned routes pick the version by position, keep response schemas for bodyless routes, send JSON bodies with `JSON.stringify` (JSON strings included), stop double-encoding URLs, and omit `Content-Type` when there is no body.
- AsyncAPI: plain JSON Schema payloads are normalized to object-form schemas, non-Draft-7 dialects are wrapped in a Multi Format Schema Object, channel and message `$ref`s are JSON-pointer escaped, and empty channel addresses or ones containing `?`/`#` are rejected.
- WebSocket schema parsing treats only an `undefined` schema as "no schema".
