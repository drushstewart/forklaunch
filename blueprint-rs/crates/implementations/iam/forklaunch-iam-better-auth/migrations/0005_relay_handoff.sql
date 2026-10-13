-- Relay is an optional generated extension. Its nonce claim uses the same
-- tenant-bound encrypted unique-index and audit transaction as auth records.
ALTER TABLE fl_better_auth.records DROP CONSTRAINT records_model_check;
ALTER TABLE fl_better_auth.records ADD CONSTRAINT records_model_check CHECK(model IN (
    'user','session','account','verification','jwks','organization','member',
    'invitation','team','teamMember','organizationRole','relaySessionHandoff'
));
