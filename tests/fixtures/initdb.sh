#!/usr/bin/env sh
set -e

FIXTURES_DIR="$(dirname "$0")"
echo "Loading Martin test fixtures into '$PGDATABASE' as user '$PGUSER' from '$FIXTURES_DIR'"

env

psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/TileBBox.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/table_source.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/table_source_multiple_geom.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/function_source.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/function_source_query_params.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points1_source.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points2_source.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points3857_source.sql
psql -v ON_ERROR_STOP=1 -f $FIXTURES_DIR/points_empty_srid_source.sql
