-- Run as migration owner, never the application runtime role.
CREATE TABLE fieldnotes_notes (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL, title text NOT NULL, body text NOT NULL,
 revision bigint NOT NULL DEFAULT 1 CHECK(revision>0), created_by text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 insight_excerpt text, insight_word_count bigint CHECK(insight_word_count>=0)
);
CREATE INDEX ON fieldnotes_notes(workspace_id,created_at,id);
CREATE TABLE fieldnotes_outbox (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL, note_id uuid NOT NULL, revision bigint NOT NULL,
 kind text NOT NULL, payload text NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 processed_at timestamptz, applied_at timestamptz
);
CREATE INDEX ON fieldnotes_outbox(created_at) WHERE processed_at IS NULL;
CREATE TABLE fieldnotes_events (
 id bigserial PRIMARY KEY, workspace_id uuid NOT NULL, note_id uuid NOT NULL, revision bigint NOT NULL,
 kind text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ON fieldnotes_events(workspace_id,id);
ALTER TABLE fieldnotes_notes ENABLE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_notes FORCE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_outbox ENABLE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_outbox FORCE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_events FORCE ROW LEVEL SECURITY;
CREATE POLICY notes_tenant ON fieldnotes_notes TO fieldnotes_api
 USING (workspace_id::text=current_setting('fieldnotes.tenant_id',true))
 WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true));
CREATE POLICY outbox_tenant ON fieldnotes_outbox TO fieldnotes_api
 USING (workspace_id::text=current_setting('fieldnotes.tenant_id',true))
 WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true));
CREATE POLICY events_tenant ON fieldnotes_events TO fieldnotes_api
 USING (workspace_id::text=current_setting('fieldnotes.tenant_id',true))
 WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true));
CREATE POLICY outbox_dispatch ON fieldnotes_outbox TO fieldnotes_worker USING(true) WITH CHECK(true);
GRANT SELECT,INSERT,UPDATE,DELETE ON fieldnotes_notes TO fieldnotes_api;
GRANT SELECT,INSERT ON fieldnotes_outbox TO fieldnotes_api;
GRANT UPDATE(applied_at) ON fieldnotes_outbox TO fieldnotes_api;
GRANT SELECT,INSERT ON fieldnotes_events TO fieldnotes_api;
GRANT USAGE,SELECT ON SEQUENCE fieldnotes_events_id_seq TO fieldnotes_api;
GRANT SELECT ON fieldnotes_outbox TO fieldnotes_worker;
GRANT UPDATE(processed_at) ON fieldnotes_outbox TO fieldnotes_worker;
