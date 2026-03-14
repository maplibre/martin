#!/bin/bash
set -e

# Natural Earth 10m data (mirror; replace with alternative if unavailable)
NE_BASE="https://naciscdn.org/naturalearth/10m/cultural"

echo "Downloading Natural Earth 10m countries..."
wget -q -O "${HOME}/ne_10m_admin_0_countries.zip" "${NE_BASE}/ne_10m_admin_0_countries.zip" || {
  echo "Warning: naciscdn failed, trying naturalearthdata.com..."
  wget -q -O "${HOME}/ne_10m_admin_0_countries.zip" "https://www.naturalearthdata.com/download/10m/cultural/ne_10m_admin_0_countries.zip" || exit 1
}
unzip -o -q "${HOME}/ne_10m_admin_0_countries.zip" -d "${HOME}/ne_countries"
COUNTRIES_SHP=$(find "${HOME}/ne_countries" -name "ne_10m_admin_0_countries.shp" | head -1)

echo "Downloading Natural Earth 10m roads..."
wget -q -O "${HOME}/ne_10m_roads.zip" "${NE_BASE}/ne_10m_roads.zip" || {
  echo "Warning: naciscdn failed, trying naturalearthdata.com..."
  wget -q -O "${HOME}/ne_10m_roads.zip" "https://www.naturalearthdata.com/download/10m/cultural/ne_10m_roads.zip" || exit 1
}
unzip -o -q "${HOME}/ne_10m_roads.zip" -d "${HOME}/ne_roads"
ROADS_SHP=$(find "${HOME}/ne_roads" -name "ne_10m_roads.shp" | head -1)

geomColumn=geom

# Countries: staging then final table with lowercase columns and SRID 3857
echo "Importing countries..."
ogr2ogr -f PostgreSQL PG:dbname=db -nln ne_countries_staging -nlt MULTIPOLYGON -lco GEOMETRY_NAME="${geomColumn}" "${COUNTRIES_SHP}"
psql -U postgres -d db -v ON_ERROR_STOP=1 <<'EOSQL'
CREATE TABLE countries AS
  SELECT
    row_number() OVER ()::integer AS id,
    COALESCE("NAME", "name", '')::text AS name,
    COALESCE("POP_EST", "pop_est", 0)::numeric AS pop_est,
    COALESCE("CONTINENT", "continent", '')::text AS continent,
    ST_Transform(geom, 3857) AS geom
  FROM ne_countries_staging;
CREATE INDEX idx_countries_geom ON countries USING GIST(geom);
DROP TABLE ne_countries_staging;
EOSQL

# Roads: staging then final table
echo "Importing roads..."
ogr2ogr -f PostgreSQL PG:dbname=db -nln ne_roads_staging -nlt LINESTRING -lco GEOMETRY_NAME="${geomColumn}" "${ROADS_SHP}"
psql -U postgres -d db -v ON_ERROR_STOP=1 <<'EOSQL'
CREATE TABLE ne_10m_roads AS
  SELECT
    COALESCE("name", "NAME", '')::text AS name,
    COALESCE("type", "TYPE", '')::text AS type,
    COALESCE("continent", "CONTINENT", '')::text AS continent,
    ST_Transform(geom, 3857) AS geom
  FROM ne_roads_staging;
CREATE INDEX idx_ne_10m_roads_geom ON ne_10m_roads USING GIST(geom);
DROP TABLE ne_roads_staging;
EOSQL

echo "Natural Earth import done."
