import pg from "pg";
import { migrateWorkspaces } from "../../migrations/workspaces.js";
export class AuthRepository {
  readonly pool: pg.Pool;
  constructor(databaseUrl: string) {
    this.pool = new pg.Pool({
      connectionString: databaseUrl,
      options: "-c search_path=auth,public",
      max: 10,
    });
  }
  async initialize() {
    await migrateWorkspaces(this.pool);
  }
  async findUser(email: string) {
    const result = await this.pool.query(
      'SELECT id FROM auth."user" WHERE email=$1',
      [email],
    );
    return result.rows[0]?.id as string | undefined;
  }
  async listWorkspaces(userId: string) {
    return (
      await this.pool.query(
        "SELECT w.id,w.name,m.role FROM auth.workspaces w JOIN auth.memberships m ON m.workspace_id=w.id WHERE m.user_id=$1 ORDER BY w.name",
        [userId],
      )
    ).rows;
  }
  async membership(workspace: string, userId: string) {
    return (
      await this.pool.query(
        "SELECT role FROM auth.memberships WHERE workspace_id=$1 AND user_id=$2",
        [workspace, userId],
      )
    ).rows[0]?.role as string | undefined;
  }
  async seedWorkspaces(ids: string[]) {
    const shared = "11111111-1111-4111-8111-111111111111",
      alice = "22222222-2222-4222-8222-222222222222",
      bob = "33333333-3333-4333-8333-333333333333";
    await this.pool.query(
      "INSERT INTO auth.workspaces(id,name) VALUES($1,$2),($3,$4),($5,$6) ON CONFLICT DO NOTHING",
      [shared, "Shared workspace", alice, "Alice private", bob, "Bob private"],
    );
    for (const [workspace, user, role] of [
      [shared, ids[0], "admin"],
      [shared, ids[1], "viewer"],
      [alice, ids[0], "editor"],
      [bob, ids[1], "admin"],
    ])
      await this.pool.query(
        "INSERT INTO auth.memberships(workspace_id,user_id,role) VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        [workspace, user, role],
      );
  }
  close() {
    return this.pool.end();
  }
}
