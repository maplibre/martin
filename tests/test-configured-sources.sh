#!/usr/bin/env bash
set -euo pipefail

TEST_OUT_DIR="$(dirname "$0")/output/configured"
mkdir -p "$TEST_OUT_DIR"

test_pbf()
{
  FILENAME="$TEST_OUT_DIR/$1"
  URL=$2

  echo "Testing $(basename "$FILENAME") from $URL"
  curl -sS --fail-with-body "$URL" > "$FILENAME"
  ./tests/vtzero-check "$FILENAME"
  ./tests/vtzero-show "$FILENAME" > "$FILENAME.txt"
}

curl -sS --fail-with-body "localhost:3000/index.json" | jq --sort-keys -e > "$TEST_OUT_DIR/catalog.json"

test_pbf "tbl_0_0_0"  "localhost:3000/public.table_source/0/0/0.pbf"
test_pbf "cmp_0_0_0"  "localhost:3000/public.points1,public.points2/0/0/0.pbf"
test_pbf "fnc_0_0_0"  "localhost:3000/rpc/public.function_source/0/0/0.pbf"
test_pbf "fnc2_0_0_0" "localhost:3000/rpc/public.function_source_query_params/0/0/0.pbf?token=martin"
