import type { RequestHandler } from "express";
import { fromNodeHeaders } from "better-auth/node";
import type { Auth } from "../../domain/services/auth.service.js";
import type { AuthRepository } from "../../persistence/repositories/auth.repository.js";
export function listWorkspaces(
  auth: Auth,
  repository: AuthRepository,
): RequestHandler {
  return async (req, res) => {
    try {
      const result = await auth.api.getSession({
        headers: fromNodeHeaders(req.headers),
        query: { disableCookieCache: true },
        returnHeaders: true,
      });
      for (const cookie of result.headers.getSetCookie())
        res.append("Set-Cookie", cookie);
      res.set("Cache-Control", "no-store");
      if (!result.response) {
        res.status(401).json({ error: "unauthorized" });
        return;
      }
      res.json({
        workspaces: await repository.listWorkspaces(result.response.user.id),
      });
    } catch {
      res.status(503).json({ error: "authentication unavailable" });
    }
  };
}
