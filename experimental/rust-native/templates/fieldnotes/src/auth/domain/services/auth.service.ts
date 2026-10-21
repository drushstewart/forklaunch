import { betterAuth } from "better-auth";
import { migrateBetterAuth } from "../../migrations/better_auth.js";
import type pg from "pg";
import type { AuthConfig } from "../../config.js";
export async function createAuth(config: AuthConfig, pool: pg.Pool) {
  const options = {
    database: pool,
    secret: config.authSecret,
    baseURL: config.authUrl,
    trustedOrigins: [config.origin],
    emailAndPassword: { enabled: true },
    session: { cookieCache: { enabled: false } },
    advanced: { useSecureCookies: config.origin.startsWith("https:") },
  };
  await migrateBetterAuth(options);
  return betterAuth(options);
}
export type Auth = Awaited<ReturnType<typeof createAuth>>;
