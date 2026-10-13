-- Record replacement, reindexing and FK cascades resolve claims by owner.
CREATE INDEX unique_claims_record ON fl_better_auth.unique_claims(tenant_id,model,id);
