# blueprint-rs

Rust blueprint crates for ForkLaunch.

This workspace mirrors the TypeScript `blueprint` style with shared core primitives, contract and interface crates, SDK generation, and reference implementations for IAM, billing, and workers. IAM provides an optional PostgreSQL backend with tenant RLS, Argon2id credentials, persistent sessions, an async Axum adapter, and generated modules. The database worker provides an optional PostgreSQL queue implementing async leases, acknowledgments, retry backoff, crash recovery, and failed-job retention. Both integrations use the `postgres` feature. The database worker also provides a `runtime` feature with a polling runner, lease renewal, bounded queue operations, and graceful drain; newly generated PostgreSQL workers use both features.

The Redis worker provides `leased::RedisLeasedQueue` behind its `redis-client` feature, with atomic claims and batches, delivery leases, renewal, retries, and crash recovery. It uses the same async queue interface and runner as PostgreSQL.

Generic service storage, batch worker handlers, generated Redis/Kafka worker loops, provider clients, billing persistence, Kafka worker backends, and distributed signals remain tracked in [the implementation plan](../plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md). See [Rust project documentation](../docs/adding-projects/rust.md) for setup and current boundaries.

## Commands

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps
```

## Package Policy

- Interface crates define contracts and traits.
- Implementation crates depend on interfaces, not the other way around.
- Optional integrations should be feature-gated at the implementation crate that owns them.
- Generated TypeScript and Rust SDKs should come from framework contract artifacts.
