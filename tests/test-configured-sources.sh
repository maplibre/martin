#!/usr/bin/env bash
set -euo pipefail

curl -sS "localhost:3000/index.json" | jq -e
curl -sS "localhost:3000/public.table_source/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.points1,public.points2/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source_query_params/0/0/0.pbf?token=martin" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
