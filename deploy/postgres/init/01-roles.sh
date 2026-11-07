#!/bin/sh
# Runs once when the Postgres data volume is first created (docker-entrypoint-initdb.d).
# Creates the RLS-bound application role and the warehouse login.
# Migrations run as $POSTGRES_USER (table owner); services log in as members
# of secretary_app, which has NOBYPASSRLS and owns nothing.
set -eu

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<EOSQL
CREATE ROLE secretary_app NOLOGIN NOBYPASSRLS;
CREATE ROLE secretary_warehouse LOGIN NOBYPASSRLS PASSWORD '${WAREHOUSE_DB_PASSWORD}' IN ROLE secretary_app;
EOSQL
