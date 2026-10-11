CREATE TABLE fl_better_auth.records (
    tenant_id text NOT NULL,
    model text NOT NULL CHECK(model IN ('user','session','account','verification','jwks','organization','member','invitation','team','teamMember','organizationRole')),
    id text NOT NULL,
    ciphertext text NOT NULL,
    sequence bigint GENERATED ALWAYS AS IDENTITY,
    PRIMARY KEY(tenant_id,model,id)
);
CREATE TABLE fl_better_auth.unique_claims (
    tenant_id text NOT NULL,
    model text NOT NULL,
    id text NOT NULL,
    field text NOT NULL,
    blind_index text NOT NULL,
    PRIMARY KEY(tenant_id,model,field,blind_index),
    FOREIGN KEY(tenant_id,model,id) REFERENCES fl_better_auth.records(tenant_id,model,id) ON DELETE CASCADE
);
CREATE TABLE fl_better_auth.audit (
    id uuid PRIMARY KEY,
    tenant_id text NOT NULL,
    model text NOT NULL,
    record_id text NOT NULL,
    actor_id text,
    correlation_id text NOT NULL,
    operation text NOT NULL CHECK(operation IN ('create','update','delete')),
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE fl_better_auth.records ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_better_auth.records FORCE ROW LEVEL SECURITY;
CREATE POLICY records_tenant ON fl_better_auth.records
    USING(tenant_id=current_setting('forklaunch.tenant_id',true))
    WITH CHECK(tenant_id=current_setting('forklaunch.tenant_id',true));
ALTER TABLE fl_better_auth.unique_claims ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_better_auth.unique_claims FORCE ROW LEVEL SECURITY;
CREATE POLICY claims_tenant ON fl_better_auth.unique_claims
    USING(tenant_id=current_setting('forklaunch.tenant_id',true))
    WITH CHECK(tenant_id=current_setting('forklaunch.tenant_id',true));
ALTER TABLE fl_better_auth.audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_better_auth.audit FORCE ROW LEVEL SECURITY;
CREATE POLICY audit_tenant ON fl_better_auth.audit
    USING(tenant_id=current_setting('forklaunch.tenant_id',true))
    WITH CHECK(tenant_id=current_setting('forklaunch.tenant_id',true));
