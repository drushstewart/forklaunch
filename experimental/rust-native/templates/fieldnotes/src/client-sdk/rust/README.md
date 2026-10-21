# Rust Notes client

HTTP and presence client implementations are generated from the shared contracts
during compilation. This is an independently addressable Cargo package coordinated by the root
workspace and its shared lockfile.

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
application workspace coordinates dependencies and its shared lockfile.
