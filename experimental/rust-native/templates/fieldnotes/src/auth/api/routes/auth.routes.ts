import express from "express";
import { toNodeHandler } from "better-auth/node";
import type { AuthDependencies } from "../../registrations.js";
import { listWorkspaces } from "../controllers/workspace.controller.js";
import { resolveSession } from "../controllers/session.controller.js";

export function registerRoutes(
  app: express.Express,
  { auth, repository, config }: AuthDependencies,
) {
  // Better Auth owns the unparsed body and cookie/CSRF processing.
  app.all("/api/auth/{*path}", toNodeHandler(auth));
  app.get("/api/workspaces", listWorkspaces(auth, repository));
  app.post(
    "/internal/session",
    express.json({ limit: "2kb" }),
    resolveSession(auth, repository, config.internalSecret),
  );
}
