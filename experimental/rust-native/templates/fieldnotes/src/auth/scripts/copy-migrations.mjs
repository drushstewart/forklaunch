import { cp, mkdir } from "node:fs/promises";
await mkdir("dist/migrations", { recursive: true });
await cp("migrations/001-workspaces.sql", "dist/migrations/001-workspaces.sql");
