-- Service reads acknowledge the same append-only tenant-scoped audit ledger.
-- No content or ciphertext is included in these bounded operation receipts.
ALTER TABLE fieldnotes_compliance_access ADD COLUMN operation_id uuid;
ALTER TABLE fieldnotes_compliance_access ADD COLUMN purpose text CHECK (purpose IS NULL OR length(purpose) BETWEEN 1 AND 128);
ALTER POLICY compliance_access_tenant ON fieldnotes_compliance_access
 USING (workspace_id::text=current_setting('fieldnotes.tenant_id',true) AND (actor_kind='user' OR (actor_kind='service' AND actor_id='service:notes')))
 WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true) AND (actor_kind='user' OR (actor_kind='service' AND actor_id='service:notes')));
CREATE POLICY compliance_access_worker ON fieldnotes_compliance_access
 FOR INSERT TO fieldnotes_worker WITH CHECK (workspace_id::text=current_setting('fieldnotes.tenant_id',true) AND actor_kind='service' AND actor_id='service:insights');
GRANT INSERT ON fieldnotes_compliance_access TO fieldnotes_worker;
