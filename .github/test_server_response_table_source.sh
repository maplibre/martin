#!/usr/bin/env bash
# This bash script tests a martin endpoint plugged into a test database against a set of expected http responses.
# The first argument of this script is expected to be the url of a martin instance.

MARTIN_URL=$1

curl "${MARTIN_URL}/index.json" | jq -e
curl "${MARTIN_URL}/public.table_source/0/0/0.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/6/38/20.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/12/2476/1280.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/13/4952/2560.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/14/9904/5121.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/20/633856/327787.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.table_source/21/1267712/655574.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf