# blueprint-rs

Rust blueprint crates for ForkLaunch.

This workspace mirrors the TypeScript `blueprint` style with shared core primitives, contract and interface crates, SDK generation, and in-memory implementations for IAM, billing, and workers. The sample service runs through the framework Axum adapter. Production persistence, provider clients, and queue integrations remain tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md).

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

## Package Policy

- Interface crates define contracts and traits.
- Implementation crates depend on interfaces, not the other way around.
- Optional integrations should be feature-gated at the implementation crate that owns them.
- Generated TypeScript and Rust SDKs should come from framework contract artifacts.
