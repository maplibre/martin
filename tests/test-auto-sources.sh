#!/usr/bin/env bash
set -euo pipefail

TEST_OUT_DIR="$(dirname "$0")/output/auto"
mkdir -p "$TEST_OUT_DIR"

test_pbf()
{
  FILENAME="$TEST_OUT_DIR/$1.pbf
  URL=$2

  echo "Testing $(basename "$FILENAME") from $URL"
  curl -sS --fail-with-body "$URL" > "$FILENAME"
  ./tests/vtzero-check "$FILENAME"
  ./tests/vtzero-show "$FILENAME" > "$FILENAME.txt"
}

>&2 echo "Test catalog"
curl -sS --fail-with-body http://localhost:3000/index.json | jq --sort-keys -e > "$TEST_OUT_DIR/catalog.json"

>&2 echo "Test server response for table source"
test_pbf tbl_0_0_0              http://localhost:3000/public.table_source/0/0/0.pbf
test_pbf tbl_6_38_20            http://localhost:3000/public.table_source/6/38/20.pbf
test_pbf tbl_12_2476_1280       http://localhost:3000/public.table_source/12/2476/1280.pbf
test_pbf tbl_13_4952_2560       http://localhost:3000/public.table_source/13/4952/2560.pbf
test_pbf tbl_14_9904_5121       http://localhost:3000/public.table_source/14/9904/5121.pbf
test_pbf tbl_20_633856_327787   http://localhost:3000/public.table_source/20/633856/327787.pbf
test_pbf tbl_21_1267712_655574  http://localhost:3000/public.table_source/21/1267712/655574.pbf

>&2 echo "Test server response for composite source"
test_pbf cmp_0_0_0              http://localhost:3000/public.table_source,public.points1,public.points2/0/0/0.pbf
test_pbf cmp_6_38_20            http://localhost:3000/public.table_source,public.points1,public.points2/6/38/20.pbf
test_pbf cmp_12_2476_1280       http://localhost:3000/public.table_source,public.points1,public.points2/12/2476/1280.pbf
test_pbf cmp_13_4952_2560       http://localhost:3000/public.table_source,public.points1,public.points2/13/4952/2560.pbf
test_pbf cmp_14_9904_5121       http://localhost:3000/public.table_source,public.points1,public.points2/14/9904/5121.pbf
test_pbf cmp_20_633856_327787   http://localhost:3000/public.table_source,public.points1,public.points2/20/633856/327787.pbf
test_pbf cmp_21_1267712_655574  http://localhost:3000/public.table_source,public.points1,public.points2/21/1267712/655574.pbf

>&2 echo "Test server response for function source"
test_pbf fnc_0_0_0              http://localhost:3000/rpc/public.function_source/0/0/0.pbf
test_pbf fnc_6_38_20            http://localhost:3000/rpc/public.function_source/6/38/20.pbf
test_pbf fnc_12_2476_1280       http://localhost:3000/rpc/public.function_source/12/2476/1280.pbf
test_pbf fnc_13_4952_2560       http://localhost:3000/rpc/public.function_source/13/4952/2560.pbf
test_pbf fnc_14_9904_5121       http://localhost:3000/rpc/public.function_source/14/9904/5121.pbf
test_pbf fnc_20_633856_327787   http://localhost:3000/rpc/public.function_source/20/633856/327787.pbf
test_pbf fnc_21_1267712_655574  http://localhost:3000/rpc/public.function_source/21/1267712/655574.pbf
test_pbf fnc_0_0_0_token        http://localhost:3000/rpc/public.function_source_query_params/0/0/0.pbf?token=martin

>&2 echo "Test server response for table source with different SRID"
test_pbf points3857_srid_0_0_0  http://localhost:3000/public.points3857/0/0/0.pbf

>&2 echo "Test server response for table source with empty SRID"
echo "IGNORING: This test is currently failing, and has been failing for a while"
echo "IGNORING:   " test_pbf points_empty_srid_0_0_0  http://localhost:3000/public.points_empty_srid/0/0/0.pbf
