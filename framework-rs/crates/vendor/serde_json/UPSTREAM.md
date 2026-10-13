# serde_json 1.0.151: literal map keys

Source: https://crates.io/crates/serde_json/1.0.151 (MIT OR Apache-2.0).
Both upstream licenses and source tests are retained.

The sole runtime change is in `src/value/de.rs`: `KeyClassifier` requests a unit
enum key and accepts its variant as a literal map key. Real JSON object keys and
owned/borrowed `Value` map keys use that enum path. Serde's private number/raw
marker key deserializers forward enum requests to their string visitor, retaining
the internal marker protocol. This separates ordinary object data from internal
number/raw tokens without changing numeric precision or exposing new wire tags.

The framework, blueprint and generated application workspace roots patch
`serde_json` to this directory so Axum, rmcp and persistence share one patched
`Value` type. Standalone external consumers must apply the same workspace patch;
registry publication and upstream adoption are separate release requirements.
The patch is intentionally limited to JSON deserialization; arbitrary third-party
Serde formats are not a new compatibility claim.

Validation: the initial isolated patch against 1.0.150 passed 151 integration
tests and 105 doctests; the final baseline is 1.0.151 to match the BullMQ protocol
pin. ForkLaunch regression tests cover literal marker keys in
raw JSON, typed nested fields, owned/borrowed Value, exact numbers, RawValue,
request parsing, MCP and encrypted persistence. See the parity-plan receipt for
integration results and remaining JSON limits.
