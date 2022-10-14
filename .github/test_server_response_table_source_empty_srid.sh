#!/usr/bin/env bash
# This bash script tests a martin endpoint plugged into a test database against a set of expected http responses.
# The first argument of this script is expected to be the url of a martin instance.

MARTIN_URL=$1

curl "${MARTIN_URL}/public.points_empty_srid/0/0/0.pbf" > table_source.pbf
./tests/vtzero-check table_source.pbf
./tests/vtzero-show table_source.pbf