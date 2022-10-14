#!/usr/bin/env bash
# This bash script tests a martin endpoint plugged into a test database against a set of expected http responses.
# The first argument of this script is expected to be the url of a martin instance.

MARTIN_URL=$1

curl "${MARTIN_URL}/rpc/public.function_source/0/0/0.pbf" > function_source0.pbf
./tests/vtzero-check function_source0.pbf
./tests/vtzero-show function_source0.pbf

curl "${MARTIN_URL}/rpc/public.function_source/6/38/20.pbf" > function_source6.pbf
./tests/vtzero-check function_source6.pbf
./tests/vtzero-show function_source6.pbf

curl "${MARTIN_URL}/rpc/public.function_source/12/2476/1280.pbf" > function_source12.pbf
./tests/vtzero-check function_source12.pbf
./tests/vtzero-show function_source12.pbf

curl "${MARTIN_URL}/rpc/public.function_source/13/4952/2560.pbf" > function_source13.pbf
./tests/vtzero-check function_source13.pbf
./tests/vtzero-show function_source13.pbf

curl "${MARTIN_URL}/rpc/public.function_source/14/9904/5121.pbf" > function_source14.pbf
./tests/vtzero-check function_source14.pbf
./tests/vtzero-show function_source14.pbf

curl "${MARTIN_URL}/rpc/public.function_source/20/633856/327787.pbf" > function_source20.pbf
./tests/vtzero-check function_source20.pbf
./tests/vtzero-show function_source20.pbf

curl "${MARTIN_URL}/rpc/public.function_source/21/1267712/655574.pbf" > function_source21.pbf
./tests/vtzero-check function_source21.pbf
./tests/vtzero-show function_source21.pbf

curl "${MARTIN_URL}/rpc/public.function_source_query_params/0/0/0.pbf?token=martin" > function_source_query_params.pbf
./tests/vtzero-check function_source_query_params.pbf
./tests/vtzero-show function_source_query_params.pbf