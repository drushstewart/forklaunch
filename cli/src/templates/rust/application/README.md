# {{{name}}}

{{{description}}}

## Development

```sh
cargo build --workspace
cargo test --workspace
cargo run -p <service-name>
```

Services expose `/health`, `/openapi.json`, and a Scalar UI at `/docs`. Set `HOST` and `PORT` to configure the HTTP listener. Services stop gracefully on Ctrl-C or SIGTERM.

## Contracts and SDKs

```sh
forklaunch openapi export --path .
forklaunch sdk generate --path . --target all --from contract
forklaunch sdk generate --path . --target all --from contract --check
```

Commit `.forklaunch/framework-rs` with this application: the bundled framework source makes Cargo dependencies portable across machines and container builds. Initialization preserves existing source files.

Service persistence and worker jobs start as in-memory examples. Implement your database repositories, migrations, and queue adapters before deploying application workloads.
