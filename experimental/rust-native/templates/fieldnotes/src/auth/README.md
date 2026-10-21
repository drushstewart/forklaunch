# Auth service

The authentication sidecar uses the same module boundaries as the TypeScript service templates. Better Auth owns credentials, sessions, cookies and its schema; this service owns workspace membership and the internal authorization endpoint.

- `server.ts` composes HTTP middleware/routes and starts the server when executed directly.
- `bootstrapper.ts` initializes the service and optionally seeds the demo accounts.
- `registrations.ts` constructs dependencies and orders Better Auth migrations before workspace migrations.
- `api/routes/` registers endpoints; `api/controllers/` handles requests and responses.
- `domain/services/` configures Better Auth; `domain/schemas/` validates internal session inputs.
- `persistence/repositories/` owns SQL access; `persistence/seeders/` creates demo data through Better Auth.
- `migrations/` owns the membership SQL and delegates the identity schema to Better Auth's migration runner.
- `index.ts` exports the public surface without starting a server or opening a database connection.
- `__test__/` contains unit tests. `tests/envelope-vector.mjs` remains the Rust-to-Node interoperability acceptance helper.

## Project commands

Run `npm ci` before these commands. The local Compose environment supplies runtime secrets and database settings; shell-based development needs the same variables exported.

| Command                                   | Purpose                                                                     |
| ----------------------------------------- | --------------------------------------------------------------------------- |
| `npm run dev`                             | Run with source watching                                                    |
| `npm run build`                           | Compile TypeScript and copy the service-owned SQL migration                 |
| `npm start`                               | Run the compiled server after building                                      |
| `npm run typecheck`                       | Check production TypeScript without emitting                                |
| `npm run lint` / `npm run lint:fix`       | ESLint checks and fixes                                                     |
| `npm run format` / `npm run format:check` | Prettier formatting and verification                                        |
| `npm test`                                | Run the service unit tests                                                  |
| `npm run migrate:up`                      | Apply Better Auth and workspace migrations without starting HTTP or seeding |
| `npm run seed`                            | Initialize schemas and create the demo accounts/workspaces                  |
| `npm run clean`                           | Remove compiled output                                                      |

Startup keeps the same order: Better Auth migrations, workspace migrations, optional demo seed, HTTP listener. The internal session route remains read-only, checks durable sessions on every request, requires the internal secret, and never refreshes cookies. Public workspace requests retain Better Auth refresh cookies.

Workspace SQL is idempotent initialization; this sidecar does not claim reversible migration history or generated down migrations.
