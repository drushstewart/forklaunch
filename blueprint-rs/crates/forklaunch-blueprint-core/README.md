# Blueprint core

`BlueprintError` carries an optional opaque typed diagnostic context through
`with_diagnostic_context` and `diagnostic_context`. Cloning retains that context;
Debug, Display, equality and public `details` exclude it. Transports continue
selecting their existing wire fields explicitly. A private marker type can
preserve internal error provenance without trusting caller-supplied JSON details.

This is a source compatibility change for external struct literals or exhaustive
struct destructuring. Construct errors with `BlueprintError::new(code, message)`
and `.with_detail(...)`; read the existing public fields or use a pattern with
`..` rather than naming every field. Repository callers already use constructors.
The opaque context is not a serialization or public diagnostic payload API.

With `instance-gateway`, `preview_model_gateway::PreviewModelGatewayClient`
provides the separate local-preview model capability. Construct it with an
`InstanceGatewayClient` or trusted preview environment settings. `models()`
returns a strict one-model catalog and issued expiration/request/unit ceilings;
these are not remaining usage, monthly token budgets, billing units, or BAA
claims. Model IDs use the source's bounded ASCII alphabet. Expiration is a safe
integer in Unix milliseconds, strictly future and at most 15 minutes away.
`with_clock` is a trusted catalog-validation clock; HMAC timestamps and fresh
nonces continue to use the existing transport independently.

Catalog responses have an exact 8192-byte raw limit, checked against advertised
length and incrementally across chunks before parsing. Invalid or expired
catalogs return sanitized status 502; gateway refusals preserve status and
positive finite retry hints without response text, and network send failures
retain unknown status. The new per-request response-limit methods can only
tighten an existing transport bound; default clients keep their previous limits
and error codes. Redirects and automatic retries remain disabled.

`chat_completion(&serde_json::Value)` posts opaque JSON to the fixed
`/models/chat/completions` route and returns JSON unchanged, including absent
`created` and additional provider fields. The relay owns request validation.
There is no streaming method, provider credential access, or public managed
model-client compatibility claim. This client must receive preview-only instance
credentials from the isolated preview relay configuration.

Preview chat decoding follows browser UTF-8 replacement/BOM handling and rounds
finite integers outside the JavaScript safe range to IEEE-754 values. The native
JSON value representation does not support overflowing numbers (such as `1e400`)
or lone UTF-16 surrogates; these responses fail decoding. JSON serialization
spelling is not promised to match JavaScript byte for byte.


With `instance-gateway`, `model_gateway::ModelGatewayClient` provides managed
`models()` and nonstreaming `chat_completion(&Value)` requests over the existing
instance-signed transport. It loads trusted managed settings through `from_env`
or accepts an explicitly configured transport. Both methods retain opaque JSON,
including extra fields and omitted `created`, matching the TypeScript unchecked
response casts. Monthly budget, usage and BAA fields are platform claims rather
than locally verified guarantees. Streaming and typed chat DTOs remain separate
work; the preview client's stricter catalog continues to use its own API.

Successful bodies have a one-MiB raw bound, including chunked responses. Browser
UTF-8 replacement/BOM handling and finite large-integer rounding match the preview
chat decoder. Overflowing numbers and lone UTF-16 surrogates remain unsupported.
Refusals preserve status and positive finite retry hints while excluding remote
response text. Every request uses a fresh HMAC nonce; redirects and automatic
retries stay disabled. TLS/loopback policy and trusted credentials are inherited
from `InstanceGatewayClient`.
