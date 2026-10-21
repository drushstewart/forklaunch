CREATE TABLE fieldnotes_attachments (
 id uuid PRIMARY KEY, workspace_id uuid NOT NULL, note_id uuid NOT NULL,
 metadata text NOT NULL, size bigint NOT NULL CHECK(size>=0 AND size<=1048576),
 state text NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','ready','deleted')),
 created_at timestamptz NOT NULL DEFAULT now(), expires_at timestamptz NOT NULL DEFAULT (clock_timestamp()+interval '60 seconds')
);
CREATE INDEX ON fieldnotes_attachments(workspace_id,note_id,state);
CREATE INDEX ON fieldnotes_attachments(state,expires_at);
ALTER TABLE fieldnotes_attachments ENABLE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_attachments FORCE ROW LEVEL SECURITY;
CREATE POLICY attachment_tenant ON fieldnotes_attachments TO fieldnotes_api
 USING(workspace_id::text=current_setting('fieldnotes.tenant_id',true))
 WITH CHECK(workspace_id::text=current_setting('fieldnotes.tenant_id',true));
CREATE POLICY attachment_worker_read ON fieldnotes_attachments FOR SELECT TO fieldnotes_worker USING(true);
CREATE POLICY attachment_worker_delete ON fieldnotes_attachments FOR DELETE TO fieldnotes_worker
 USING(state='deleted' OR (state='pending' AND expires_at<clock_timestamp()-interval '1 hour'));
GRANT SELECT,INSERT,UPDATE ON fieldnotes_attachments TO fieldnotes_api;
GRANT SELECT,DELETE ON fieldnotes_attachments TO fieldnotes_worker;
