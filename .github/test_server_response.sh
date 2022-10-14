#!/usr/bin/env bash
# This bash script tests a martin endpoint plugged into a test database against a set of expected http responses.
# The first argument of this script is expected to be the url of a martin instance.

MARTIN_URL=$1

curl "${MARTIN_URL}/index.json" | jq -e

curl "${MARTIN_URL}/public.table_source/0/0/0.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf

curl "${MARTIN_URL}/public.points1,public.points2/0/0/0.pbf" > composite_source.pbf
./tests/vtzero-check composite_source.pbf
./tests/vtzero-show composite_source.pbf

curl "${MARTIN_URL}/rpc/public.function_source/0/0/0.pbf" > function_source.pbf
./tests/vtzero-check function_source.pbf
./tests/vtzero-show function_source.pbf

curl "${MARTIN_URL}/rpc/public.function_source_query_params/0/0/0.pbf?token=martin" > function_source_query_params.pbf
./tests/vtzero-check function_source_query_params.pbf
./tests/vtzero-show function_source_query_params.pbf