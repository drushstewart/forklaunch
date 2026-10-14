-- Optional TOTP rows use the existing encrypted records, tenant RLS, audit and
-- blind unique userId claim. Existing user ciphertext is not rewritten.
ALTER TABLE fl_better_auth.records DROP CONSTRAINT records_model_check;
ALTER TABLE fl_better_auth.records ADD CONSTRAINT records_model_check CHECK(model IN (
    'user','session','account','verification','jwks','organization','member',
    'invitation','team','teamMember','organizationRole','relaySessionHandoff','twoFactor'
));
