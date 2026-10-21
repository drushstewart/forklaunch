#!/bin/sh
set -eu
# Execute inside the owned Postgres container. Runtime role passwords stay in its environment.
export PGPASSWORD="$API_DB_PASSWORD"
psql -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U fieldnotes_api -d "$POSTGRES_DB" <<'SQL'
BEGIN;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname=current_user AND (rolsuper OR rolbypassrls)) THEN RAISE EXCEPTION 'runtime role bypasses RLS'; END IF;
 IF EXISTS(SELECT 1 FROM pg_class WHERE relname='fieldnotes_notes' AND relowner=(SELECT oid FROM pg_roles WHERE rolname=current_user)) THEN RAISE EXCEPTION 'runtime owns notes'; END IF;
 IF EXISTS(SELECT 1 FROM fieldnotes_notes) THEN RAISE EXCEPTION 'missing tenant revealed notes'; END IF;
END $$;
SELECT set_config('fieldnotes.tenant_id','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',true) AS scope \gset
INSERT INTO fieldnotes_notes(id,workspace_id,title,body,created_by) VALUES('bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','v1.synthetic.title','v1.synthetic.body','rls-test');
DO $$ BEGIN
 IF (SELECT count(*) FROM fieldnotes_notes WHERE id='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb')<>1 THEN RAISE EXCEPTION 'own row missing'; END IF;
END $$;
SELECT set_config('fieldnotes.tenant_id','cccccccc-cccc-4ccc-8ccc-cccccccccccc',true) AS scope \gset
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM fieldnotes_notes WHERE id='bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb') THEN RAISE EXCEPTION 'foreign row visible'; END IF;
 BEGIN
  INSERT INTO fieldnotes_notes(id,workspace_id,title,body,created_by) VALUES('dddddddd-dddd-4ddd-8ddd-dddddddddddd','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','synthetic','synthetic','rls-test');
  RAISE EXCEPTION 'foreign write permitted';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM 1 FROM auth.memberships;
  RAISE EXCEPTION 'API may read auth membership database';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM fieldnotes_notes) THEN RAISE EXCEPTION 'pooled connection retained tenant'; END IF;
END $$;
SQL
export PGPASSWORD="$WORKER_DB_PASSWORD"
psql -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U fieldnotes_worker -d "$POSTGRES_DB" <<'SQL'
DO $$ BEGIN
 BEGIN
  PERFORM 1 FROM fieldnotes_notes;
  RAISE EXCEPTION 'worker may read notes';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  UPDATE fieldnotes_outbox SET payload='modified' WHERE false;
  RAISE EXCEPTION 'worker may alter encrypted payload';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  DELETE FROM fieldnotes_attachments WHERE state='ready';
  -- RLS must silently filter every ready row even when no rows exist in this initial proof.
  IF FOUND THEN RAISE EXCEPTION 'worker deleted ready attachment'; END IF;
 END;
END $$;
SQL
export PGPASSWORD="$POSTGRES_PASSWORD"
psql -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U postgres -d "$POSTGRES_DB" <<'SQL'
BEGIN;
SET LOCAL ROLE fieldnotes_api;
SELECT set_config('fieldnotes.tenant_id','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',true) AS scope \gset
INSERT INTO fieldnotes_attachments(id,workspace_id,note_id,metadata,size,state,expires_at) VALUES
 ('eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb','synthetic',0,'ready',now()-interval '2 hours'),
 ('ffffffff-ffff-4fff-8fff-ffffffffffff','aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa','bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb','synthetic',0,'pending',now()-interval '2 hours');
SET LOCAL ROLE fieldnotes_worker;
DO $$ BEGIN
 DELETE FROM fieldnotes_attachments WHERE id='eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee';
 IF FOUND THEN RAISE EXCEPTION 'worker deleted ready attachment'; END IF;
 IF NOT EXISTS(SELECT 1 FROM fieldnotes_attachments WHERE id='eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee') THEN RAISE EXCEPTION 'ready attachment missing'; END IF;
 DELETE FROM fieldnotes_attachments WHERE id='ffffffff-ffff-4fff-8fff-ffffffffffff';
 IF NOT FOUND THEN RAISE EXCEPTION 'expired pending cleanup rejected'; END IF;
END $$;
ROLLBACK;
SQL
printf '%s\n' 'PASS runtime-role RLS isolation, write check, auth-table separation, worker grants, and transaction-local scope reset; fixture transaction rolled back'
