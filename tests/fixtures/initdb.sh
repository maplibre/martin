#!/usr/bin/env bash
set -euo pipefail

FIXTURES_DIR="$(dirname "$0")"
echo "Loading Martin test fixtures into '$PGDATABASE' as user '$PGUSER' from '$FIXTURES_DIR'"


psql -P pager=off -v ON_ERROR_STOP=1 -c "CREATE EXTENSION IF NOT EXISTS postgis;"
# see https://github.com/postgis/docker-postgis/issues/187
psql -P pager=off -v ON_ERROR_STOP=1 -c "DROP SCHEMA IF EXISTS tiger CASCADE;"
psql -P pager=off -v ON_ERROR_STOP=1 -t -c "select version();"
psql -P pager=off -v ON_ERROR_STOP=1 -t -c "select PostGIS_Full_Version();"

psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/TileBBox.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/table_source.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/table_source_multiple_geom.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/function_source.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/function_source_query_params.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points1_source.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points2_source.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points3857_source.sql
psql -e -P pager=off -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points_empty_srid_source.sql
