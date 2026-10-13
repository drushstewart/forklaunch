CREATE TABLE fl_better_auth.lookup_claims (
    tenant_id text NOT NULL,
    model text NOT NULL,
    id text NOT NULL,
    field text NOT NULL,
    blind_index text NOT NULL,
    sequence bigint NOT NULL,
    PRIMARY KEY(tenant_id,model,field,blind_index,sequence,id),
    FOREIGN KEY(tenant_id,model,id) REFERENCES fl_better_auth.records(tenant_id,model,id) ON DELETE CASCADE
);
CREATE INDEX lookup_claims_record ON fl_better_auth.lookup_claims(tenant_id,model,id);
ALTER TABLE fl_better_auth.lookup_claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_better_auth.lookup_claims FORCE ROW LEVEL SECURITY;
CREATE POLICY lookup_claims_tenant ON fl_better_auth.lookup_claims
    USING(tenant_id=current_setting('forklaunch.tenant_id',true))
    WITH CHECK(tenant_id=current_setting('forklaunch.tenant_id',true));
