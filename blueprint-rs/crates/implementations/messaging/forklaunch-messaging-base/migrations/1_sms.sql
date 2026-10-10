CREATE TABLE fl_messaging.sms_records (
    tenant_id text NOT NULL,
    id text NOT NULL,
    provider_message_id text,
    recipient_indexes text[] NOT NULL,
    encrypted_record text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    retention_anonymized_at timestamptz,
    PRIMARY KEY (tenant_id, id),
    UNIQUE (tenant_id, provider_message_id)
);
CREATE INDEX sms_recipient_indexes ON fl_messaging.sms_records USING gin(recipient_indexes);
CREATE TABLE fl_messaging.pii_read_audit (
    id uuid PRIMARY KEY,
    tenant_id text NOT NULL,
    record_id text NOT NULL,
    actor_id text,
    correlation_id text NOT NULL,
    operation text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
ALTER TABLE fl_messaging.sms_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_messaging.sms_records FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_policy ON fl_messaging.sms_records USING
    (tenant_id = current_setting('forklaunch.tenant_id', true)) WITH CHECK
    (tenant_id = current_setting('forklaunch.tenant_id', true));
ALTER TABLE fl_messaging.pii_read_audit ENABLE ROW LEVEL SECURITY;
ALTER TABLE fl_messaging.pii_read_audit FORCE ROW LEVEL SECURITY;
CREATE POLICY tenant_policy ON fl_messaging.pii_read_audit USING
    (tenant_id = current_setting('forklaunch.tenant_id', true)) WITH CHECK
    (tenant_id = current_setting('forklaunch.tenant_id', true));
