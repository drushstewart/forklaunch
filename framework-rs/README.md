# framework-rs

Rust-native ForkLaunch framework crates.

This workspace implements contract registration, validation primitives, an Axum HTTP adapter, OpenAPI and AsyncAPI generation, MCP tool metadata, SDK code generation, and infrastructure interfaces. Optional runtime integrations are feature-gated. Production integration and cross-language parity work remains tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md).

The CLI can generate runnable Rust backends with bundled framework source. See [Rust backend generation](../docs/adding-projects/rust.md) for setup, contract exports, SDKs, and current boundaries.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

## Package Policy

- Workspace package metadata lives in the root `Cargo.toml`.
- Optional integrations should be feature-gated at the crate that owns the integration.
- Core contract types should stay independent of any specific HTTP framework.
- `forklaunch-axum` is the first adapter, not the framework core.
