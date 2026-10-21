#!/bin/sh
set -eu
# Run in a generated Notes app after scripts/check-api.mjs completes a new job.
# Only metadata aggregates are returned; note contents and ciphertext stay private.
test -f src/notes/migrations/006_service-job-access.sql
docker compose exec -T postgres psql -X -v ON_ERROR_STOP=1 -U postgres -d fieldnotes <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS (
   SELECT 1 FROM fieldnotes_compliance_access input
   JOIN fieldnotes_compliance_access result USING (operation_id,workspace_id,resource_id)
   WHERE input.actor_kind='service' AND input.actor_id='service:insights'
     AND result.actor_kind='service' AND result.actor_id='service:notes'
     AND input.classification='pii' AND result.classification='pii'
     AND input.field='payload' AND result.field='payload'
     AND input.purpose IS NOT NULL AND result.purpose IS NOT NULL
 ) THEN RAISE EXCEPTION 'Expected correlated worker input and API result audit receipts'; END IF;
 IF EXISTS (
   SELECT 1 FROM fieldnotes_compliance_access a
   WHERE row_to_json(a)::text LIKE '%confidential acceptance marker%'
      OR row_to_json(a)::text LIKE '%encrypted attachment acceptance marker%'
 ) THEN RAISE EXCEPTION 'Plaintext canary leaked into audit ledger'; END IF;
END $$;
SELECT actor_id, count(*) AS classified_service_reads
 FROM fieldnotes_compliance_access WHERE actor_kind='service'
 GROUP BY actor_id ORDER BY actor_id;
SQL
echo 'PASS correlated durable worker/API payload audits without plaintext canaries'
