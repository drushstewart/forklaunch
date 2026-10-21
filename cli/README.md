# forklaunch

Launch faster with forklaunch - a CLI for scaffolding TypeScript, Rust, and mixed service workspaces.

## Usage

```bash
forklaunch init application my-app
```

```bash
forklaunch init service my-service
```

```bash
forklaunch init library my-library
```

## Rust workspaces

```bash
forklaunch init application my-app --language rust --path ./my-app
forklaunch init service notes --language rust --path ./my-app
forklaunch init worker insights --language rust --path ./my-app
```

New Rust workspaces use `src/<service>`, `src/<worker>`, `src/core/<library>`,
and `src/client-sdk/{typescript,rust}`. Each Rust project has its own Cargo
manifest and Make targets. Services separate routes, controllers, schemas,
domain services, persistence, and startup; `main.rs` and `lib.rs` are entry/export
surfaces. Native Axum routers use ForkLaunch transport security and telemetry
wrappers. Service-owned contract crates generate TypeScript SDKs during Cargo
builds. Registered legacy projects retain their existing paths.

The complete encrypted Notes review preset includes Rust Notes and Insights,
a Better Auth sidecar, generated clients, a web UI, service-owned migrations,
and local Docker Compose infrastructure:

```bash
forklaunch init application fieldnotes --language mixed --preset fieldnotes --sdk-targets all --path ./fieldnotes
cd fieldnotes
python3 scripts/setup.py
docker compose up --build -d
```

Generation writes sources only and refuses an existing destination. Follow the
generated README for local credentials, certificate trust, verification scripts,
and the framework/application security boundary. Framework sources are vendored
under `vendor/framework` during this review; generating an app does not publish
crates or deploy services.
