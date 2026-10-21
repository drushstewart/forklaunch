-- The cross-language contract uses UTC milliseconds (Rust DateTime / TS Date).
-- Normalize existing review timestamps explicitly; the wire codec never truncates.
ALTER TABLE fieldnotes_notes
 ALTER COLUMN created_at TYPE timestamptz(3) USING date_trunc('milliseconds', created_at),
 ALTER COLUMN updated_at TYPE timestamptz(3) USING date_trunc('milliseconds', updated_at);
ALTER TABLE fieldnotes_attachments
 ALTER COLUMN created_at TYPE timestamptz(3) USING date_trunc('milliseconds', created_at);
-- The existing UPDATE trigger advances cache_version, preventing a pre-migration
-- encrypted cached projection from returning timestamps with finer precision.
UPDATE fieldnotes_notes SET updated_at = updated_at;
