-- Signed internal callbacks must reject replay across all API replicas.
CREATE TABLE fieldnotes_internal_nonces (
    nonce text PRIMARY KEY CHECK (length(nonce) <= 1024),
    expires_at timestamptz NOT NULL
);
CREATE INDEX fieldnotes_internal_nonces_expiry ON fieldnotes_internal_nonces(expires_at);
REVOKE ALL ON fieldnotes_internal_nonces FROM PUBLIC;
GRANT SELECT,INSERT,DELETE ON fieldnotes_internal_nonces TO fieldnotes_api;
