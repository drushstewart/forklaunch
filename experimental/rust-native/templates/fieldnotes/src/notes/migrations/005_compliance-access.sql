-- Audit writes acknowledge a separate committed transaction before plaintext is
-- released. Do not add a note FK: audit survives erasure and business rollback.
CREATE TABLE fieldnotes_compliance_access (
 id uuid PRIMARY KEY,
 workspace_id uuid NOT NULL,
 resource_id uuid NOT NULL,
 actor_kind text NOT NULL CHECK (actor_kind IN ('user','service')),
 actor_id text NOT NULL CHECK (length(actor_id) BETWEEN 1 AND 256),
 entity text NOT NULL CHECK (length(entity) BETWEEN 1 AND 128),
 field text NOT NULL CHECK (length(field) BETWEEN 1 AND 128),
 classification text NOT NULL CHECK (classification IN ('pii','phi','pci')),
 reason text NOT NULL CHECK (length(reason) BETWEEN 1 AND 256),
 occurred_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX ON fieldnotes_compliance_access(workspace_id,occurred_at);
ALTER TABLE fieldnotes_compliance_access ENABLE ROW LEVEL SECURITY;
ALTER TABLE fieldnotes_compliance_access FORCE ROW LEVEL SECURITY;
CREATE POLICY compliance_access_tenant ON fieldnotes_compliance_access TO fieldnotes_api
 USING (workspace_id::text=current_setting('fieldnotes.tenant_id',true))
 WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true));
REVOKE ALL ON fieldnotes_compliance_access FROM PUBLIC;
GRANT INSERT ON fieldnotes_compliance_access TO fieldnotes_api;
-- Runtime cannot read, modify, or delete audit records. Retention/export is an
-- explicit privileged operational policy, not a public API capability.
