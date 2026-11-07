#!/bin/sh
# Runs once when the Postgres data volume is first created (docker-entrypoint-initdb.d).
# Keycloak (D14) keeps its own database and login role on the same server; no
# CodeSecretary service ever reads it. For an existing volume, run the same
# statements by hand (see deploy/keycloak/README.md).
set -eu

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<EOSQL
CREATE ROLE keycloak LOGIN PASSWORD '${KEYCLOAK_DB_PASSWORD}';
CREATE DATABASE keycloak OWNER keycloak;
EOSQL
