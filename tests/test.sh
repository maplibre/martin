#!/usr/bin/env bash
set -euo pipefail

# TODO: use  --fail-with-body  to get the response body on failure
CURL=${CURL:-curl -sSf}
DATABASE_URL="${DATABASE_URL:-postgres://postgres@localhost/db}"
MARTIN_BUILD="${MARTIN_BUILD:-cargo build}"
MARTIN_PORT="${MARTIN_PORT:-3000}"
MARTIN_URL="http://localhost:${MARTIN_PORT}"
MARTIN_BIN="${MARTIN_BIN:-cargo run -- --listen-addresses localhost:${MARTIN_PORT}}"

function wait_for_martin {
    # Seems the --retry-all-errors option is not available on older curl versions, but maybe in the future we can just use this:
    # timeout -k 20s 20s curl --retry 10 --retry-all-errors --retry-delay 1 -sS "$MARTIN_URL/health"
    PROCESS_ID=$1
    echo "Waiting for Martin ($PROCESS_ID) to start..."
    for i in {1..30}; do
        if curl -sSf "$MARTIN_URL/health" 2>/dev/null >/dev/null; then
            echo "Martin is up!"
            curl -s "$MARTIN_URL/health"
            return
        fi
        if ps -p $PROCESS_ID > /dev/null ; then
            echo "Martin is not up yet, waiting..."
            sleep 1
        else
            echo "Martin died!"
            ps au
            lsof -i || true
            exit 1
        fi
    done
    echo "Martin did not start in time"
    ps au
    lsof -i || true
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
  URL="$MARTIN_URL/$2"

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

ARG=(--default-srid 900913)
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee test_log_1.txt &
PROCESS_ID=`jobs -p`

{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

TEST_OUT_DIR="$(dirname "$0")/output/auto"
mkdir -p "$TEST_OUT_DIR"

>&2 echo "Test catalog"
$CURL "$MARTIN_URL/catalog" | jq --sort-keys -e | tee "$TEST_OUT_DIR/catalog.json"

>&2 echo "Test server response for table source"
test_pbf tbl_0_0_0                table_source/0/0/0
test_pbf tbl_6_38_20              table_source/6/38/20
test_pbf tbl_12_2476_1280         table_source/12/2476/1280
test_pbf tbl_13_4952_2560         table_source/13/4952/2560
test_pbf tbl_14_9904_5121         table_source/14/9904/5121
test_pbf tbl_20_633856_327787     table_source/20/633856/327787
test_pbf tbl_21_1267712_655574    table_source/21/1267712/655574

>&2 echo "Test server response for composite source"
test_pbf cmp_0_0_0                table_source,points1,points2/0/0/0
test_pbf cmp_6_38_20              table_source,points1,points2/6/38/20
test_pbf cmp_12_2476_1280         table_source,points1,points2/12/2476/1280
test_pbf cmp_13_4952_2560         table_source,points1,points2/13/4952/2560
test_pbf cmp_14_9904_5121         table_source,points1,points2/14/9904/5121
test_pbf cmp_20_633856_327787     table_source,points1,points2/20/633856/327787
test_pbf cmp_21_1267712_655574    table_source,points1,points2/21/1267712/655574

>&2 echo "Test server response for function source"
test_pbf fnc_0_0_0                function_zxy_query/0/0/0
test_pbf fnc_6_38_20              function_zxy_query/6/38/20
test_pbf fnc_12_2476_1280         function_zxy_query/12/2476/1280
test_pbf fnc_13_4952_2560         function_zxy_query/13/4952/2560
test_pbf fnc_14_9904_5121         function_zxy_query/14/9904/5121
test_pbf fnc_20_633856_327787     function_zxy_query/20/633856/327787
test_pbf fnc_21_1267712_655574    function_zxy_query/21/1267712/655574
test_pbf fnc_0_0_0_token          function_zxy_query_test/0/0/0?token=martin

>&2 echo "Test server response for different function call types"
test_pbf fnc_zoom_xy_6_38_20      function_zoom_xy/6/38/20
test_pbf fnc_zxy_6_38_20          function_zxy/6/38/20
test_pbf fnc_zxy2_6_38_20         function_zxy2/6/38/20
test_pbf fnc_zxy_query_6_38_20    function_zxy_query/6/38/20
test_pbf fnc_zxy_row_6_38_20      function_zxy_row/6/38/20
test_pbf fnc_zxy_row2_6_38_20     function_Mixed_Name/6/38/20
test_pbf fnc_zxy_row_key_6_38_20  function_zxy_row_key/6/38/20

>&2 echo "Test server response for table source with different SRID"
test_pbf points3857_srid_0_0_0    points3857/0/0/0

>&2 echo "Test server response for table source with empty SRID"
echo "IGNORING: This test is currently failing, and has been failing for a while"
echo "IGNORING:   " test_pbf points_empty_srid_0_0_0  points_empty_srid/0/0/0

kill_process $PROCESS_ID
grep -e ' ERROR ' -e ' WARN ' test_log_1.txt && exit 1


echo "------------------------------------------------------------------------------------------------------------------------"
echo "Test pre-configured Martin"
set -x

ARG=(--config tests/config.yaml "$DATABASE_URL")
$MARTIN_BIN "${ARG[@]}" 2>&1 | tee test_log_2.txt &
PROCESS_ID=`jobs -p`
{ set +x; } 2> /dev/null
trap "kill -9 $PROCESS_ID 2> /dev/null || true" EXIT
wait_for_martin $PROCESS_ID

TEST_OUT_DIR="$(dirname "$0")/output/configured"
mkdir -p "$TEST_OUT_DIR"

>&2 echo "Test catalog"
$CURL "$MARTIN_URL/catalog" | jq --sort-keys -e | tee "$TEST_OUT_DIR/catalog.json"

test_pbf tbl_0_0_0   table_source/0/0/0
test_pbf cmp_0_0_0   points1,points2/0/0/0
test_pbf fnc_0_0_0   function_zxy_query/0/0/0
test_pbf fnc2_0_0_0  function_zxy_query_test/0/0/0?token=martin

kill_process $PROCESS_ID
grep -e ' ERROR ' -e ' WARN ' test_log_2.txt && exit 1

>&2 echo "All integration tests have passed"
