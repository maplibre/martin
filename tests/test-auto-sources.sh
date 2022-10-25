#!/usr/bin/env bash
set -euo pipefail

>&2 echo "Test server response for table source"

curl -sS "localhost:3000/index.json" | jq -e
curl -sS "localhost:3000/public.table_source/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/6/38/20.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/12/2476/1280.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/13/4952/2560.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/14/9904/5121.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/20/633856/327787.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source/21/1267712/655574.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf

>&2 echo "Test server response for composite source"

curl -sS "localhost:3000/public.table_source,public.points1,public.points2/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/6/38/20.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/12/2476/1280.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/13/4952/2560.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/14/9904/5121.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/20/633856/327787.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/public.table_source,public.points1,public.points2/21/1267712/655574.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf

>&2 echo "Test server response for function source"

curl -sS "localhost:3000/rpc/public.function_source/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/6/38/20.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/12/2476/1280.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/13/4952/2560.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/14/9904/5121.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/20/633856/327787.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source/21/1267712/655574.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
curl -sS "localhost:3000/rpc/public.function_source_query_params/0/0/0.pbf?token=martin" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf

>&2 echo "Test server response for table source with different SRID"

curl -sS "localhost:3000/public.points3857/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf

>&2 echo "Test server response for table source with empty SRID"

curl -sS "localhost:3000/public.points_empty_srid/0/0/0.pbf" > tmp.pbf
./tests/vtzero-check tmp.pbf
./tests/vtzero-show tmp.pbf
