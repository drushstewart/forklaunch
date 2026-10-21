CREATE TABLE IF NOT EXISTS auth.workspaces(id uuid PRIMARY KEY,name text NOT NULL);
CREATE TABLE IF NOT EXISTS auth.memberships(workspace_id uuid NOT NULL REFERENCES auth.workspaces(id),user_id text NOT NULL REFERENCES auth."user"(id) ON DELETE CASCADE,role text NOT NULL CHECK(role IN ('viewer','editor','admin')),PRIMARY KEY(workspace_id,user_id));
