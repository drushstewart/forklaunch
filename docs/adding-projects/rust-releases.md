# Rust crate releases

The [Rust Crate Release workflow](../../.github/workflows/rust_release.yml)
publishes only the 13 framework crates, including their three required protocol
forks, in dependency order. Local folders retain short names; published names use
`forklaunch-`. Run `python3 scripts/rust_release.py plan` for the release inventory.
`plan --scope all` also inventories the 28 blueprint packages for development;
publication rejects that scope and any report containing blueprint packages.
Sample crates marked `publish = false` are excluded. Pull requests validate packages;
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
their default features, with JSON Schema/referencing protocol defaults disabled, checked bounded regex,
core build/schema/mapper/PostgreSQL/OTLP features and Axum MCP/native features enabled. It roundtrips literal JSON protocol-marker keys and runs real HTTP/MCP
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
and then publishes the framework crates. Retrying the same release replaces its
parser/verification assets only after the tagged source and artifact checks pass.

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

Publication is not atomic across crates. The publisher waits for each version in
the registry, compares its downloaded archive with the verified package (allowing
only Cargo's generated lockfile to differ), and writes `publication.json` after
each success. Reruns resume by verifying existing versions; mismatched or yanked
versions stop the release. Cargo uses its configured credential provider, including
`cargo login`, registry-specific environment tokens or CI credentials. The initial public publication,
external installation and remaining optional adapter/deployment gates remain separate from
having a tested publishing pipeline.

## Native framework release acceptance

Encryption is implemented in core; native worker support belongs to the existing
blueprint worker-interface crate. Blueprint crates stay local in this framework-only
release; generated workers retain their implementation snapshot and consume the
published framework dependencies.
Publication order includes runtime and build dependencies; development-only test
backedges (for example a derive macro testing its public facade) do not create a
publication cycle. Development dependency paths and versions are still validated.

A prepared archive or passing dependency inventory is not a public release. Before
changing CLI templates to registry dependencies, verify all of the following:

- Exact framework crate versions are available in the intended registry, including
  the HTTP native feature used by generated apps. Blueprint worker adapters remain local.
- A fresh CLI-generated app builds without `vendor/framework` and without workspace
  source paths. Natural local dependency aliases may retain the published
  `forklaunch-` package names.
- Contract compilation generates OpenAPI, TypeScript and Rust SDKs. Docker starts
  HTTP/MCP and authenticates SSE/WebSocket and worker traffic.
- The required JSON parser patch is supplied and verified. The current patch means
  a fully vendor-free consumer remains outstanding even after framework crates
  are published; the existing root patch must not silently be dropped.

The current CLI intentionally retains its source snapshot until those gates pass.
`--allow-dirty` is for local validation and cannot produce a publishable receipt.

Current publication readiness is recorded in
[`RUST-PUBLISH-READINESS.json`](../../plan/RUST-PUBLISH-READINESS.json). The current
release inventory is 13 framework packages; older 41- or 53-package staging proofs
do not validate this restricted publication graph. Registry-mode worker generation still retains
its blueprint worker-interface snapshot, in addition to the required parser patch.

## Explicit application dependency modes

Initialization defaults to a portable framework source snapshot while publication
is pending. To consume an existing release, pass `--framework-version X.Y.Z` to
`forklaunch init application NAME`. Application initialization is language-neutral;
choose `--language rust` on subsequent service/worker commands. Both SDK targets
are the default. The saved preference pins direct framework dependencies to that
exact version when a Rust project is added, preserving features and local aliases
without copying the full framework snapshot. There are no application presets.
Generation does not assert the version exists. The required parser remains as
`vendor/serde_json` with a root patch; this is not yet a fully vendor-free mode.

For host framework development, use `--framework-path /absolute/path/framework-rs`
instead. This deliberately produces host-local path dependencies. Before attaching
Rust, the CLI requires an application-owned `Dockerfile.rust` integrating that
source; managed images do not automatically supply a local checkout. The two flags are mutually
exclusive. Source selection is recorded in `.forklaunch/framework-dependencies.toml`
and applied to subsequently generated native Cargo manifests. Blueprint module
implementations remain development snapshots; their framework dependencies use
the selected source.

After selecting either mode, run `cargo generate-lockfile` and commit the resulting
lock before `cargo build --locked` or Docker startup. A source-snapshot lock cannot
prove registry resolution and is removed during initialization. Never substitute
a local path build for a clean registry-install release gate.

### Why renaming the parser is insufficient

Publishing the patched parser as a different crate does not redirect transitive
upstream `serde_json` dependencies. Axum, MCP adapters and SQL persistence would
then use a different `Value` type and potentially different marker-key semantics.
A Cargo patch entry for a differently named package does not transparently replace
all dependencies on the upstream package. Removing the required patch therefore
needs upstream adoption or an explicitly tested boundary/type migration across
those consumers. A package rename alone is not an acceptable release fix.

## HTTP SDK compiler consistency

Native service builds emit OpenAPI and discrete TypeScript exports under the
service's `sdk/typescript` directory. Its `sdk/rust` Cargo package compiles against
the same private contract host and includes generated Rust source from `OUT_DIR`.
The optional client aggregate depends only on these local SDK packages. Current contract artifacts carry
`sdkCompiler: forklaunch.sdk.http.v1`. For those artifacts, CLI SDK generation
invokes the same core compiler linked into the CLI, without spawning a Cargo host
or importing the running application.
Generation consumes the supplied artifact, including path/query/header schemas,
status codes and media types; it does not rebuild service contracts implicitly.
The CLI preserves project-local SDK packages and uses the same compiler for
explicit artifact-based exports.
`--check` stages expected output and reports drift without rewriting SDK files.
Older artifacts without a compiler marker continue through the legacy compiler.

The compiler host needs Cargo and the configured dependency sources, including
this release's parser patch. Its first invocation may compile dependencies;
subsequent runs reuse Cargo's cache. Do not interpret successful local host
compilation as evidence that unpublished registry versions exist.
