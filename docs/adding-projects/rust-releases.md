# Rust crate releases

The [Rust Crate Release workflow](../../.github/workflows/rust_release.yml)
packages the 49 public framework, blueprint and protocol-fork crates in dependency
order. The three sample crates are private. Pull requests validate packages;
manual runs also validate by default. Uploading requires selecting an existing
`rust-vVERSION` tag and explicitly enabling the `publish` input.

## Local validation

Python 3.12+, stable Rust and Node 22 are required. Use an output directory outside
the checkout so release artifacts do not dirty the source tree:

```sh
python3 -m unittest discover -s scripts -p test_rust_release.py -v
python3 scripts/rust_release.py plan
python3 scripts/rust_release.py prepare --output /tmp/forklaunch-rust-release
python3 scripts/rust_release.py verify \
  --output /tmp/forklaunch-rust-release \
  --target-dir /tmp/forklaunch-rust-release-target
```

Preparation requires a new output directory and a clean checkout. For local
development, `prepare --allow-dirty` is available; a dirty release cannot publish.
The report contains the commit, ordered package inventory and SHA-256 checksums.
Preparation uses Cargo's versioned path dependency packaging support, excludes
lockfiles, and defers build verification until all unpublished dependencies can
be installed together. See [Cargo dependency locations](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations).

Verification serves a loopback sparse registry containing only the owned packages.
It changes owned dependency registry metadata in staging copies, preserves source
files and original archives, and obtains external dependencies from crates.io.
Every public crate must resolve from the test registry. A separate consumer builds
their default features, with the three protocol forks' defaults disabled and Axum
MCP enabled. It roundtrips literal JSON protocol-marker keys and runs real HTTP/MCP
discovery, invocation, Origin rejection, listener configuration, startup rejection
and graceful shutdown checks. This is an installation and startup check; optional
adapter features and runtime behavior retain their own parity tests.

## Required JSON parser artifact

Applications must apply the supplied Serde JSON 1.0.151 patch at their workspace
root. Cargo does not inherit a dependency's root patch. The patch preserves literal
`$serde_json::private::Number` and `$serde_json::private::RawValue` object keys under
feature unification; using unpatched upstream Serde JSON changes those semantics.
See the [boundary audit](../../plan/RUST-JSON-BOUNDARY-AUDIT.md) and
[upstream patch provenance](../../framework-rs/crates/vendor/serde_json/UPSTREAM.md).

Download `serde_json-1.0.151-forklaunch-literal-keys.tar.gz` and `release.json` from
the matching Rust GitHub release, compare the archive's SHA-256 with
`requiredRootPatch.sha256`, and extract it into your workspace's `vendor` directory:

```toml
[patch.crates-io]
serde_json = { path = "vendor/serde_json" }
```

The validator uses this separate artifact, without referring to repository source
paths. The patched parser is not uploaded under the upstream `serde_json` crate
name. Until a public release exists, preparation produces the same required
artifact locally; registry availability is not implied by a passing local check.

## Publishing

Create the versioned tag and corresponding public GitHub release only after release
review, configure the repository's `CARGO_REGISTRY_TOKEN` with access to the intended
crate names, then run the workflow on that tag with `publish` enabled. The workflow
first validates, uploads the parser artifact and reports to the existing release,
and then publishes the owned crates. It does not create or overwrite release assets.

The same checks can be reviewed locally without uploading:

```sh
python3 scripts/rust_release.py publish \
  --output /tmp/forklaunch-rust-release --tag rust-v0.1.0
```

Only adding `--upload` permits Cargo uploads. Preflight checks the clean checkout,
tag, commit, complete package graph, all artifact checksums and the matching
successful installation/startup receipt. Receipts are CI evidence, not signed
attestations; publish from trusted workflow artifacts. Cargo publishes sequentially
from the checked-out source, stopping at the first failure. It repackages crates
and may add lockfiles, so upload archive bytes need not equal the earlier preparation
archives. `--no-verify` avoids repeating individual package builds after the combined
registry consumer has passed. [Cargo publication behavior](https://doc.rust-lang.org/cargo/commands/cargo-publish.html)
also includes waiting for newly published versions to become available.

Publication is not atomic across crates. If a run partially publishes, inspect
crates.io and the release assets before continuing; existing versions and assets
are never silently skipped or overwritten. The initial public publication,
external installation and optional adapter/deployment gates remain separate from
having a tested publishing pipeline.
