# Notes service

Native Axum notes API. Routes and controllers live in `api/`; `registrations.rs`
composes dependencies, `bootstrapper.rs` loads startup configuration, and
`server.rs` mounts the HTTP/MCP surfaces and manages listener shutdown.

## Project tasks

Run from this project directory:

```sh
make fmt-check
make lint
make check
make test
make build
```

`make fmt` formats only this Cargo package. `lint` runs Clippy for all package
targets with warnings treated as errors. Override the Cargo command when needed:
`make CARGO='cargo +stable' test`. These targets use the local `Cargo.toml`; the
application workspace still coordinates shared dependencies and its lockfile.

`make run` launches this service with its configured environment.
