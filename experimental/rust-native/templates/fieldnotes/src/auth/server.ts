import express from "express";
import { pathToFileURL } from "node:url";
import { bootstrap } from "./bootstrapper.js";
import type { AuthDependencies } from "./registrations.js";
import { registerRoutes } from "./api/routes/auth.routes.js";

export function createServer(dependencies: AuthDependencies) {
  const app = express();
  app.disable("x-powered-by");
  app.get("/healthz", (_req, res) => res.json({ ok: true }));
  registerRoutes(app, dependencies);
  app.use((_req, res) => res.status(404).json({ error: "not found" }));
  app.use(
    (
      _error: unknown,
      _req: express.Request,
      res: express.Response,
      _next: express.NextFunction,
    ) => res.status(400).json({ error: "invalid request" }),
  );
  return app.listen(dependencies.config.port, "0.0.0.0");
}

export async function startServer() {
  const dependencies = await bootstrap();
  const server = createServer(dependencies);
  process.on("SIGTERM", () =>
    server.close(() => {
      void dependencies.repository.close().then(() => process.exit(0));
    }),
  );
  return server;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
  await startServer();
