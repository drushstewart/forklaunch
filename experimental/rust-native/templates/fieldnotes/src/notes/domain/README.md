# Notes domain and SDK contract build

The Notes service owns its schemas, domain types, services, and mappers here.
`schemas/` defines the wire DTOs; `types/`, `services/`, and `mappers/` contain
application implementation compiled through the service's `lib.rs`.

The small Cargo package in this directory compiles `../sdk.rs`, which exports
only schemas and the route contracts in `../api/contracts/`. This lets the
service's build script and both SDK compilers consume the contracts without a
circular dependency on the running server. It shares the repository workspace
and lockfile and does not introduce another runtime or deployment.

Edit schemas and route contracts to update the generated clients. Build the
Notes service to emit TypeScript SDK artifacts; building the Rust SDK generates
its Rust clients. Compiling only this contract package checks the contract types
without running the Notes service's SDK-emitting build script.

## Project tasks

From this directory, `make check`, `make lint`, `make test`, `make build`,
`make docs`, and `make fmt-check` target the contract package. `make format`
formats its source. For service implementation checks, run these commands from
`src/notes` instead. Override the Cargo command with `CARGO='cargo +stable'`
when needed.

Persistence records pass through `NotesMappers` before leaving `NotesService`,
including direct callers outside HTTP. Mapper schemas compile at registration;
invalid stored revisions, sizes, strings or timestamp precision fail with a
sanitized storage error. These checks do not replace tenant authorization,
protected-read audit acknowledgments or database transactions.
