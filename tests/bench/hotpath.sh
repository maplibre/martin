#!/usr/bin/env bash
# Runs the hotpath-instrumented martin through several small benchmarks, one server each.
# Reports land in $HOTPATH_OUTPUT_DIR/<name>.json. Usage: tests/bench/hotpath.sh [name...]
set -euo pipefail

MARTIN=${MARTIN:-target/release/martin}
PORT=${PORT:-3000}
OUT_DIR=${HOTPATH_OUTPUT_DIR:-/tmp/metrics}
WORK_DIR=$(mktemp -d)
REQUESTS=${REQUESTS:-200000}
PG_REQUESTS=${PG_REQUESTS:-20000}
HIT_REQUESTS=${HIT_REQUESTS:-1000000}
MARTIN_PID=''

mkdir -p "$OUT_DIR"
trap 'stop_martin; rm -rf "$WORK_DIR"' EXIT

urls() {
    local file="$WORK_DIR/$1_z$2.txt" z x y n
    for ((z = 0; z <= $2; z++)); do
        n=$((1 << z))
        for ((x = 0; x < n; x++)); do
            for ((y = 0; y < n; y++)); do echo "http://localhost:$PORT/$1/$z/$x/$y"; done
        done
    done > "$file"
    echo "$file"
}

start_martin() {
    local name=$1; shift
    HOTPATH_OUTPUT_FORMAT=json HOTPATH_OUTPUT_PATH="$OUT_DIR/$name.json" HOTPATH_ALLOC_SELF=true \
        "$MARTIN" --listen-addresses "0.0.0.0:$PORT" "$@" &
    MARTIN_PID=$!
    for _ in {1..120}; do
        curl -sf "http://localhost:$PORT/health" > /dev/null 2>&1 && return
        sleep 1
    done
    echo "::error::Martin failed to start for $name"
    exit 1
}

stop_martin() {
    [[ -n $MARTIN_PID ]] || return 0
    kill "$MARTIN_PID" 2> /dev/null || true
    wait "$MARTIN_PID" 2> /dev/null || true
    MARTIN_PID=''
}

load() {
    local file=$1 n=$2; shift 2
    oha --latency-correction --no-tui -n 500 --urls-from-file "$@" "$file" > /dev/null
    oha --latency-correction -n "$n" --urls-from-file "$@" "$file"
}

bench_mbtiles_miss() {
    start_martin mbtiles_miss --cache-size 0 tests/fixtures/mbtiles
    load "$(urls world_cities 6)" "$REQUESTS"
}

bench_pmtiles_miss() {
    start_martin pmtiles_miss --cache-size 0 tests/fixtures/pmtiles
    load "$(urls stamen_toner__raster_CC-BY-ODbL_z3 3)" "$REQUESTS"
}

bench_postgres_function_miss() {
    start_martin postgres_function_miss --cache-size 0
    load "$(urls function_zxy_query 6)" "$PG_REQUESTS"
}

bench_postgres_function_mlt_miss() {
    start_martin postgres_function_mlt_miss --cache-size 0
    load "$(urls function_zxy_query 6)" "$PG_REQUESTS" -H 'Accept: application/vnd.maplibre-tile'
}

bench_postgres_table_miss() {
    start_martin postgres_table_miss --cache-size 0
    load "$(urls table_source 6)" "$PG_REQUESTS"
}

bench_cache_hit() {
    start_martin cache_hit tests/fixtures/mbtiles tests/fixtures/pmtiles
    local file="$WORK_DIR/cache_hit.txt"
    {
        echo "http://localhost:$PORT/world_cities/0/0/0"
        echo "http://localhost:$PORT/png/0/0/0"
    } > "$file"
    load "$file" "$HIT_REQUESTS"
}

ALL=(mbtiles_miss pmtiles_miss postgres_function_miss postgres_function_mlt_miss postgres_table_miss cache_hit)
for name in "${@:-${ALL[@]}}"; do
    echo "::group::bench $name"
    "bench_$name"
    stop_martin
    echo "::endgroup::"
done
