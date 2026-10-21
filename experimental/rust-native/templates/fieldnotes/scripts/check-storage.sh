#!/bin/sh
set -eu
# Run after check-api.mjs, which retains an encrypted note and attachment.
docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -U postgres -d fieldnotes <<'SQL'
DO $$ BEGIN
 IF NOT EXISTS (SELECT FROM fieldnotes_notes) OR EXISTS (
   SELECT FROM fieldnotes_notes WHERE title !~ '^v[123]\.' OR body !~ '^v[123]\.'
   OR (insight_excerpt IS NOT NULL AND insight_excerpt !~ '^v[123]\.'))
 THEN RAISE EXCEPTION 'Expected encrypted notes and insights'; END IF;
 IF NOT EXISTS (SELECT FROM fieldnotes_attachments WHERE state='ready') OR EXISTS (
   SELECT FROM fieldnotes_attachments WHERE metadata !~ '^v[123]\.')
 THEN RAISE EXCEPTION 'Expected encrypted attachment metadata'; END IF;
 IF NOT EXISTS (SELECT FROM fieldnotes_outbox) OR EXISTS (
   SELECT FROM fieldnotes_outbox WHERE payload !~ '^v[123]\.')
 THEN RAISE EXCEPTION 'Expected encrypted outbox snapshots'; END IF;
 IF NOT EXISTS (SELECT FROM apalis.jobs WHERE status='Done') OR EXISTS (
   SELECT FROM apalis.jobs WHERE convert_from(job, 'UTF8') LIKE '%confidential acceptance marker%'
   OR coalesce(convert_from(job, 'UTF8')::jsonb->>'payload', '') !~ '^v[123]\.'
   OR last_result::text LIKE '%confidential acceptance marker%')
 THEN RAISE EXCEPTION 'Expected completed jobs without plaintext snapshots/results'; END IF;
END $$;
SQL
container=$(docker compose ps -q objects)
volume=$(docker inspect "$container" --format '{{range .Mounts}}{{if eq .Destination "/data"}}{{.Name}}{{end}}{{end}}')
test -n "$volume"
docker run --rm --network none --mount "type=volume,source=$volume,target=/data,readonly" alpine:latest sh -eu -c '
 test -d /data/fieldnotes/attachments
 test -n "$(find /data/fieldnotes/attachments -type f -print -quit)"
 if grep -R -q "encrypted attachment acceptance marker" /data/fieldnotes/attachments; then
   echo "Plaintext attachment marker found" >&2; exit 1
 fi
'
echo 'PASS encrypted note/insight/outbox/metadata columns, completed queue payload safety, and no plaintext attachment marker in stored objects'
