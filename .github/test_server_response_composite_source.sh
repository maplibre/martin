#!/usr/bin/env bash
# This bash script tests a martin endpoint plugged into a test database against a set of expected http responses.
# The first argument of this script is expected to be the url of a martin instance.

MARTIN_URL=$1

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/0/0/0.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/6/38/20.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/12/2476/1280.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/13/4952/2560.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/14/9904/5121.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/20/633856/327787.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/public.table_source,public.points1,public.points2/21/1267712/655574.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf