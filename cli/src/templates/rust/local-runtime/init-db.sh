#!/bin/sh
set -eu
psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" \
  --set=api_password="$API_DB_PASSWORD" \
  --set=auth_password="$AUTH_DB_PASSWORD" \
  --set=worker_password="$WORKER_DB_PASSWORD" <<'SQL'
CREATE ROLE @@SERVICE_ROLE@@ LOGIN NOBYPASSRLS PASSWORD :'api_password';
CREATE ROLE @@AUTH_ROLE@@ LOGIN NOBYPASSRLS PASSWORD :'auth_password';
CREATE ROLE @@WORKER_ROLE@@ LOGIN NOBYPASSRLS PASSWORD :'worker_password';
CREATE SCHEMA auth AUTHORIZATION @@AUTH_ROLE@@;
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
ALTER ROLE @@AUTH_ROLE@@ SET search_path TO auth;
SQL
