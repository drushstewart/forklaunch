-- Cache generations are independent of optimistic content revisions: applying an
-- insight changes the read projection without changing the editor's revision.
ALTER TABLE fieldnotes_notes ADD COLUMN cache_version bigint NOT NULL DEFAULT 1 CHECK(cache_version>0);
CREATE FUNCTION fieldnotes_note_cache_version() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 NEW.cache_version := OLD.cache_version + 1;
 RETURN NEW;
END;
$$;
CREATE TRIGGER fieldnotes_note_cache_version BEFORE UPDATE ON fieldnotes_notes
 FOR EACH ROW EXECUTE FUNCTION fieldnotes_note_cache_version();
REVOKE ALL ON FUNCTION fieldnotes_note_cache_version() FROM PUBLIC;
