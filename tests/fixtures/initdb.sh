#!/usr/bin/env bash
set -euo pipefail

FIXTURES_DIR="$(dirname "$0")"
echo -e "\n\n\n"
echo "################################################################################################"
echo "Loading Martin test fixtures into '${DATABASE_URL:-${PGDATABASE:-(local db)}}'"
echo "################################################################################################"


psql -P pager=off -v ON_ERROR_STOP=1 -c "CREATE EXTENSION IF NOT EXISTS postgis;"
# see https://github.com/postgis/docker-postgis/issues/187
psql -P pager=off -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS tiger CASCADE;"
psql -P pager=off -v ON_ERROR_STOP=1 -t -c "select version();"
psql -P pager=off -v ON_ERROR_STOP=1 -t -c "select PostGIS_Full_Version();"

# On error, make sure do delete all the tables we created
# TODO: see if we can have a fail-early service test to detect errors
trap 'echo -e "\n\n\n!!!!!!!!!!!!!!!!!!!!!!!!\n\nDropping schemas DUE TO AN ERROR!\n\n\n" && psql -c "DROP SCHEMA IF EXISTS "MixedCase" CASCADE; DROP SCHEMA IF EXISTS autodetect CASCADE;"' ERR

echo -e "\n\n\n"
echo "################################################################################################"
echo "Importing tables from $FIXTURES_DIR/tables"
echo "################################################################################################"
for sql_file in "$FIXTURES_DIR"/tables/*.sql; do
  psql -e -P pager=off -v ON_ERROR_STOP=1 -f "$sql_file"
done

echo -e "\n\n\n"
echo "################################################################################################"
echo "Importing functions from $FIXTURES_DIR/functions"
echo "################################################################################################"
for sql_file in "$FIXTURES_DIR"/functions/*.sql; do
  psql -e -P pager=off -v ON_ERROR_STOP=1 -f "$sql_file"
done

echo -e "\n\n\n"
echo "################################################################################################"
echo "Active pg_hba.conf configuration"
echo "################################################################################################"
psql -P pager=off -v ON_ERROR_STOP=1 -c "select pg_reload_conf();"
psql -P pager=off -v ON_ERROR_STOP=1 -c "select * from pg_hba_file_rules;"
