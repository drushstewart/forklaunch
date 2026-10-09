---
title: Adding Projects - Rust Backends
category: Guides
description: Generate Rust services, contracts, and TypeScript or Rust SDKs.
---

# Rust backend generation

The Rust port lives on `rohin/rust-framework`. It adds Rust services, workers, libraries, routers, contract exports, and generated SDKs to the existing CLI.

## Create and run a service

```sh
forklaunch init application my-app --language rust -D "My app" -A "My team" -L MIT
cd my-app
forklaunch init service accounts --language rust -d postgresql -D "Accounts service"
forklaunch init router users --language rust --path crates/accounts
cargo test --workspace
cargo run -p accounts
```

The service serves `/health`, `/users`, `/openapi.json`, and a Scalar documentation UI at `/docs`. `HOST`/`PORT` (or `SERVICE_HOST`/`SERVICE_PORT`) configure the listener. `DOCS_PATH`, `DOCS_SERVER_URLS`, and `DOCS_SERVER_DESCRIPTIONS` configure documentation. Ctrl-C and SIGTERM stop the HTTP server gracefully.

Network requests use Axum and the same contract handlers as `local_router`. Path parameters, decoded URL queries, headers, binary bodies, body limits, CORS, and request IDs pass through the framework adapter.

## Export contracts and SDKs

```sh
forklaunch openapi export --path .
forklaunch sdk generate --path . --target all --from contract
forklaunch sdk generate --path . --target all --from contract --check
```

Exports are written under `.forklaunch/contracts/<service>/contract.json` and `.forklaunch/openapi/<service>/openapi.json`. TypeScript output lives in `sdks/typescript`; Rust SDK crates live in `sdks/rust`. `--sdk-targets rust` on app initialization omits the TypeScript package scaffold. Rust services require generated SDKs; live TypeScript SDK mode is rejected.

`sync`, `change`, and `delete` update Cargo membership, local library dependencies, Compose services, manifest entries, and SDK references. Initialization and sync preserve existing application code. Service/worker conversion replaces the generated project shape.

## Portable source and containers

Generated applications include framework source in `.forklaunch/framework-rs`. Commit this directory with the application. Cargo uses relative dependencies, so moving the application or building it inside Docker does not require the CLI author's repository checkout. Existing bundled source is preserved when initialization is repeated.

```sh
docker compose build accounts
docker compose up accounts
```

The generated Dockerfile builds workspace binaries, and Compose selects the service binary, port, and health check. Container deployment to ECS has not yet been verified.

## Current boundaries

Generated service persistence is an in-memory example. Selecting a database records configuration; it does not create a production database repository or migrations. Generated workers execute one demonstration batch. The blueprint IAM, billing, and queue implementations also have unfinished production integrations. Full auth parity, streaming and multipart parity, cross-language conformance, crate publication, and production deployment validation remain tracked in `plan/RUST-FRAMEWORK-BLUEPRINT-IMPLEMENTATION.md`.

## Validate the port

```sh
cd framework-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ../blueprint-rs
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd ../cli
cargo test
cargo build --release
cd tests
export FORKLAUNCH_CLI="$(cd ../target/release && pwd)/forklaunch"
bash init_rust_projects.sh
for script in rust*.sh; do bash "$script"; done
```

The Rust E2E job runs generation and lifecycle scripts, checks generated TypeScript SDKs, and calls a relocated Rust service through its generated TypeScript SDK. SDK checks require TypeScript (`TSC_BIN` can point to an installed compiler); the live HTTP check also requires Node.js with built-in `fetch`.
