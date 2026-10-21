export function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Missing ${name}`);
  return value;
}
export function loadConfig() {
  const internalSecret = required("INTERNAL_AUTH_SECRET");
  if (internalSecret.length < 32) throw new Error("Internal secret too short");
  return {
    internalSecret,
    origin: new URL(required("PUBLIC_ORIGIN")).origin,
    databaseUrl: required("DATABASE_URL"),
    authSecret: required("BETTER_AUTH_SECRET"),
    authUrl: required("BETTER_AUTH_URL"),
    port: Number(process.env.PORT ?? 3001),
    seedDemo: process.env.SEED_DEMO === "true",
  };
}
export type AuthConfig = ReturnType<typeof loadConfig>;
