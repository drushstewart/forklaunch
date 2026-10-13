-- Keyset scans stay bounded even when a tenant model exceeds 4096 records.
CREATE INDEX records_tenant_model_sequence
    ON fl_better_auth.records (tenant_id, model, sequence);
