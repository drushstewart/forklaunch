# framework-rs

Rust-native ForkLaunch framework crates.

This workspace is intentionally scaffolded with minimal internal dependencies first. External runtime choices such as Axum, OpenTelemetry, Redis, S3, and SDK code generation will be added behind crate feature flags as the implementation fills in.

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
