import type { RequestHandler } from "express";
import { fromNodeHeaders } from "better-auth/node";
import type { Auth } from "../../domain/services/auth.service.js";
import type { AuthRepository } from "../../persistence/repositories/auth.repository.js";
import { timingSafeEqual } from "node:crypto";
import { parseSessionRequest } from "../../domain/schemas/session.schema.js";
export function resolveSession(
  auth: Auth,
  repository: AuthRepository,
  internalSecret: string,
): RequestHandler {
  return async (req, res) => {
    res.set("Cache-Control", "no-store");
    const supplied = Buffer.from(req.header("x-fieldnotes-internal") ?? "");
    const expected = Buffer.from(internalSecret);
    if (
      supplied.length !== expected.length ||
      !timingSafeEqual(supplied, expected)
    ) {
      res.status(401).end();
      return;
    }
    const body = parseSessionRequest(req.body);
    if (!body) {
      res.status(400).end();
      return;
    }
    const workspace = body.workspaceId;
    try {
      // Public get-session/workspace checks refresh cookies. Internal authorization is read-only,
      // checks durable sessions every request, and never drops a refresh Set-Cookie header.
      const session = await auth.api.getSession({
        headers: fromNodeHeaders(req.headers),
        query: { disableCookieCache: true, disableRefresh: true },
      });
      if (!session) {
        res.status(401).end();
        return;
      }
      const role = await repository.membership(workspace, session.user.id);
      if (!role) {
        res.status(403).end();
        return;
      }
      res.json({
        userId: session.user.id,
        workspaceId: workspace.toLowerCase(),
        role,
      });
    } catch {
      res.status(503).json({ error: "authentication unavailable" });
    }
  };
}
