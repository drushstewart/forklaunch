#!/bin/sh
set -eu
# Operational metadata only: never print encrypted job arguments or results.
docker compose exec -T postgres psql -U postgres -d fieldnotes -c '
SELECT id, job_type, status, attempts, max_attempts, run_at, done_at
FROM apalis.jobs ORDER BY run_at DESC LIMIT 50;'
