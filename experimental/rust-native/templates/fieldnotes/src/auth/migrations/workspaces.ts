import { readFile } from "node:fs/promises";
import type pg from "pg";

// Runs after Better Auth creates auth.user. Preserve the existing idempotent DDL.
export async function migrateWorkspaces(pool: pg.Pool): Promise<void> {
  const sql = await readFile(
    new URL("./001-workspaces.sql", import.meta.url),
    "utf8",
  );
  await pool.query(sql);
}
