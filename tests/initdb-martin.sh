#!/bin/sh

set -e

# Perform all actions as $POSTGRES_USER
export PGUSER="$POSTGRES_USER"

echo "Loading Martin fixtures into $POSTGRES_DB"

psql --dbname="$POSTGRES_DB" -f /fixtures/TileBBox.sql

psql --dbname="$POSTGRES_DB" -f /fixtures/table_source.sql
psql --dbname="$POSTGRES_DB" -f /fixtures/table_source_multiple_geom.sql

psql --dbname="$POSTGRES_DB" -f /fixtures/function_source.sql
psql --dbname="$POSTGRES_DB" -f /fixtures/function_source_query_params.sql

psql --dbname="$POSTGRES_DB" -f /fixtures/points1_source.sql
psql --dbname="$POSTGRES_DB" -f /fixtures/points2_source.sql
psql --dbname="$POSTGRES_DB" -f /fixtures/points3857_source.sql
psql --dbname="$POSTGRES_DB" -f /fixtures/points_empty_srid_source.sql
