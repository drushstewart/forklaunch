# blueprint-rs

Rust blueprint crates for ForkLaunch.

This workspace mirrors the TypeScript `blueprint` style with shared core primitives, interface crates, and implementation crates. The crates are currently scaffolded without external dependencies so the workspace can compile while the Rust framework contract layer is filled in.

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
