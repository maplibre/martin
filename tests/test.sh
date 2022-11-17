#!/usr/bin/env bash
set -euo pipefail

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl -sSf}
DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BUILD="${MARTIN_BUILD:-cargo +nightly build}"
MARTIN_BIN="${MARTIN_BIN:-cargo +nightly run --}"

function wait_for_martin {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS http://localhost:3000/healthz
    PROCESS_ID=$1
    echo "Waiting for Martin ($PROCESS_ID) to start..."
    for i in {1..30}; do
        if curl -sSf http://localhost:3000/healthz 2>/dev/null >/dev/null; then
            echo "Martin is up!"
            curl -s http://localhost:3000/healthz
            return
        fi
        if ps -p $PROCESS_ID > /dev/null ; then
            echo "Martin is not up yet, waiting..."
            sleep 1
        else
            echo "Martin died!"
            ps au
            lsof -i
            exit 1
        fi
    done
    echo "Martin did not start in time"
    ps au
    lsof -i
    exit 1
}

function kill_process {
    PROCESS_ID=$1
    echo "Waiting for Martin ($PROCESS_ID) to stop..."
    kill $PROCESS_ID
    for i in {1..50}; do
        if ps -p $PROCESS_ID > /dev/null ; then
            sleep 0.1
        else
            echo "Martin ($PROCESS_ID) has stopped"
            return
        fi
    done
    echo "Martin did not stop in time, killing it"
    kill -9 $PROCESS_ID
    # wait for it to die using timeout and wait
    timeout -k 1s 1s wait $PROCESS_ID || true
}

test_pbf()
{
  FILENAME="$TEST_OUT_DIR/$1.pbf"
  URL=$2

  echo "Testing $(basename "$FILENAME") from $URL"
  $CURL "$URL" > "$FILENAME"

  if [[ $OSTYPE == linux* ]]; then
    ./tests/vtzero-check "$FILENAME"
    ./tests/vtzero-show "$FILENAME" > "$FILENAME.txt"
  fi
}

curl --version

# Make sure martin is built - this way it won't timeout while waiting for it to start
# If MARTIN_BUILD is set to "-", don't build
if [[ "$MARTIN_BUILD" != "-" ]]; then
  $MARTIN_BUILD
fi


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test auto configured Martin"
set -x
$MARTIN_BIN --default-srid 900913 &
PROCESS_ID=$!
{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

TEST_OUT_DIR="$(dirname "$0")/output/auto"
mkdir -p "$TEST_OUT_DIR"

>&2 echo "Test catalog"
$CURL http://localhost:3000/index.json | jq --sort-keys -e | tee "$TEST_OUT_DIR/catalog.json"
$CURL http://localhost:3000/rpc/index.json | jq --sort-keys -e | tee "$TEST_OUT_DIR/rpc_catalog.json"

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

kill_process $PROCESS_ID


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test pre-configured Martin"
set -x
$MARTIN_BIN --config tests/config.yaml "$DATABASE_URL" &
PROCESS_ID=$!
{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

TEST_OUT_DIR="$(dirname "$0")/output/configured"
mkdir -p "$TEST_OUT_DIR"

>&2 echo "Test catalog"
$CURL http://localhost:3000/index.json | jq --sort-keys -e | tee "$TEST_OUT_DIR/catalog.json"
$CURL http://localhost:3000/rpc/index.json | jq --sort-keys -e | tee "$TEST_OUT_DIR/rpc_catalog.json"

test_pbf tbl_0_0_0  http://localhost:3000/public.table_source/0/0/0.pbf
test_pbf cmp_0_0_0  http://localhost:3000/public.points1,public.points2/0/0/0.pbf
test_pbf fnc_0_0_0  http://localhost:3000/rpc/public.function_source/0/0/0.pbf
test_pbf fnc2_0_0_0 http://localhost:3000/rpc/public.function_source_query_params/0/0/0.pbf?token=martin

kill_process $PROCESS_ID
