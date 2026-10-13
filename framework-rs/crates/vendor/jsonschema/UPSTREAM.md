# Upstream provenance

Vendored from `jsonschema 0.58.6`, MIT licensed, published source archive
`https://crates.io/api/v1/crates/jsonschema/0.58.6/download`.

Archive SHA-256: `f622c574a29fb3034a0a90645afee194dfedc75fc124228801b6e77041075bd1`.
Upstream Git commit: `55ac1664384793c5d3b20745d3f9c169dfe2c06f`.
Repository: https://github.com/Stranger6667/jsonschema

The package is renamed to `forklaunch-jsonschema-protocol` to scope compatibility
to ForkLaunch socket codecs without patching consumers' registry dependencies.
`Cargo.toml.upstream` preserves the original manifest. Unused standalone test
and benchmark targets are omitted; library unit tests remain available.

The optional `forklaunch-ajv-pointer-decoding` feature preserves literal slash
characters inside percent-encoded pointer tokens, matching AJV 8.20.0. Default
resolution retains the upstream RFC fragment behavior. Internal JSON pointers
are unchanged. The jsonschema dependency and pointer feature forward to the isolated
referencing package. Its separate optional `forklaunch-ajv-ref-siblings` feature
applies adjacent validation constraints in Drafts 4–7, matching AJV; upstream
behavior remains the default. No schema documents or literal data are rewritten.

Upstream library validation runs with `--no-default-features --features idna`
and `-- --skip validator::tests::validate_ref`. That single upstream test reads
`tests/suite/tests/draft7/ref.json`, which is absent from the published archive
(the JSON Schema Test Suite is an upstream Git submodule). All other 1,992
library tests run. ForkLaunch's own codec fixtures exercise local, recursive,
annotation and resource-scoped references independently against actual AJV.
The optional macro, Python, Ruby and HTTP retrieval adapters are outside the
ForkLaunch codec dependency profile.

Run standalone Cargo checks from the repository root with
`CARGO_TARGET_DIR=framework-rs/target` so build artifacts stay outside vendored
source and template directories.

The optional `forklaunch-static-anchor-resolution` feature keeps `$ref` to a
`$dynamicAnchor` static, while `$dynamicRef` still resolves through dynamic
scope (JSON Schema 2020-12 §8.2.3.2). Native codecs opt in. The referencing
resolver exposes `lookup_static`; the validator caches named references by
their resolved target URI and pointer, so static and dynamically rebound targets
do not share an incorrect cache entry. Pointer-reference aliases retain their
upstream identity to preserve embedded-resource and recursive-reference behavior.
A feature-gated regression covers static and dynamic references to the same
anchor; the full upstream library suite also runs with the feature enabled.
Default feature behavior remains unchanged.
