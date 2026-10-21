import { getMigrations } from "better-auth/db/migration";

// The installed Better Auth package owns its schema and migration runner.
export async function migrateBetterAuth(
  options: Parameters<typeof getMigrations>[0],
): Promise<void> {
  await (await getMigrations(options)).runMigrations();
}
