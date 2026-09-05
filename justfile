#!/usr/bin/env just --justfile

set shell := ['bash', '-c']

# Import demo sub-justfile as a module
mod demo 'demo/justfile'

# Import martin-ui sub-justfile as a module
mod ui 'martin/martin-ui/justfile'

# list of features we deem stable for release packaging
stable_features := 'contour,fonts,geojson,hillshade,lambda,mbtiles,metrics,mlt,passthrough,pmtiles,postgres,sprites,styles,tui,webui'

# How to call the current just executable. Note that just_executable() may have `\` in Windows paths, so we need to quote it.
just := quote(just_executable())
# cargo-binstall needs a workaround due to caching when used in CI
binstall_args := if env('CI', '') != '' {'--no-confirm --no-track --disable-telemetry'} else {''}
insta_test := 'cargo insta test --test-runner nextest --disable-nextest-doctest --accept --force-update-snapshots'

# if running in CI, treat warnings as errors by setting CARGO_BUILD_WARNINGS to 'deny' unless it is already set
# Use `CI=true just ci-test` to run the same tests as in GitHub CI.
# Use `just env-info` to see the current value of CARGO_BUILD_WARNINGS
ci_mode := if env('CI', '') != '' {'1'} else {''}
export CARGO_BUILD_WARNINGS := env('CARGO_BUILD_WARNINGS', if ci_mode == '1' {'deny'} else {'warn'})
export RUST_BACKTRACE := env('RUST_BACKTRACE', if ci_mode == '1' {'1'} else {'0'})

# Build in release mode by default. Set RELEASE_MODE='' to build in debug mode (used for PRs in CI to reduce build time).
# Use `RELEASE_MODE= just build-release <target>` to build in debug mode locally.
release_mode := if env('RELEASE_MODE', '1') != '' {'1'} else {''}

# Download the prebuilt maplibre_native core amalgam instead of compiling the ~1 GB C++ core from source.
# Set MLN_PRECOMPILE=0 to build maplibre_native from source instead.
export MLN_PRECOMPILE := env('MLN_PRECOMPILE', '1')
#export RUST_LOG := 'debug'
#export RUST_LOG := 'sqlx::query=info,trace'

#export DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'

# Set additional database connection parameters, e.g.   just  PGPARAMS='keepalives=0&keepalives_idle=15'  psql
PGPARAMS := ''
PGPORT := '5411'

export DATABASE_URL := ('postgres://postgres:postgres@localhost:' + PGPORT + '/db' + (if PGPARAMS != '' { '?' + PGPARAMS } else { '' }))
export CARGO_TERM_COLOR := 'always'

# Set AWS variables for testing pmtiles from S3
export AWS_SKIP_CREDENTIALS := '1'
export AWS_REGION := 'eu-central-1'

@_default:
    {{just}} --list

# Run benchmark tests
bench: fetch
    cargo bench --bench sources
    cargo bench -p martin-core --bench geojson_tiles
    open target/criterion/report/index.html

# Run HTTP requests benchmark using OHA tool. Use with `just bench-server`.
bench-http requests='10m' pg_requests='500k':  (cargo-install 'oha')
    @echo "ATTENTION: Make sure Martin was started with    just bench-server"
    @echo "Warming up..."
    oha --latency-correction -n 200            --no-tui http://localhost:3000/feature_collection_1/0/0/0 > /dev/null
    oha --latency-correction -n {{requests}}            http://localhost:3000/feature_collection_1/0/0/0
    oha --latency-correction -n 100            --no-tui http://localhost:3000/function_zxy_query/18/235085/122323 > /dev/null
    oha --latency-correction -n {{pg_requests}}         http://localhost:3000/function_zxy_query/18/235085/122323
    oha --latency-correction -n 100            --no-tui -H 'Accept: application/vnd.maplibre-tile' http://localhost:3000/function_zxy_query/18/235085/122323 > /dev/null
    oha --latency-correction -n {{pg_requests}}         -H 'Accept: application/vnd.maplibre-tile' http://localhost:3000/function_zxy_query/18/235085/122323
    oha --latency-correction -n 200            --no-tui http://localhost:3000/png/0/0/0 > /dev/null
    oha --latency-correction -n {{requests}}            http://localhost:3000/png/0/0/0
    oha --latency-correction -n 200            --no-tui http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0 > /dev/null
    oha --latency-correction -n {{requests}}            http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0

# Start release-compiled Martin server and a test database
bench-server: fetch start prepare-mbtiles
    cargo run --release -- tests/fixtures/mbtiles tests/fixtures/pmtiles tests/fixtures/geojson

# Build martin with hotpath profiling support
build-hotpath: fetch
    RUSTFLAGS="$RUSTFLAGS --cfg tokio_unstable" cargo build --release --features hotpath

# Start release-compiled Martin server with hotpath profiling (MCP on port 6771)
bench-server-hotpath: start build-hotpath prepare-mbtiles
    exec target/release/martin tests/fixtures/mbtiles tests/fixtures/pmtiles

# Run the hotpath benchmark end-to-end: start the profiled server, wait for it, drive HTTP load, shut it down. Used by the hotpath-profile CI workflow.
bench-hotpath:
    #!/usr/bin/env bash
    set -euo pipefail
    {{just}} bench-server-hotpath &
    MARTIN_PID=$!

    for i in {1..1000}; do
        if curl -sf http://localhost:3000/health > /dev/null 2>&1; then break; fi
        sleep 1
    done
    curl -sf http://localhost:3000/health > /dev/null 2>&1 || { echo "::error::Martin failed to start"; kill "$MARTIN_PID" 2>/dev/null; exit 1; }

    {{just}} bench-http 1m 100k

    kill "$MARTIN_PID" 2>/dev/null || true
    wait "$MARTIN_PID" || true

# Regenerate configs' JSON Schema, HTTP OpenAPI spec, and TS types
gen-schemas: fetch
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p schemas
    cargo build --quiet --features unstable-schemas --bin gen-schemas
    gen="${CARGO_TARGET_DIR:-target}/debug/gen-schemas"
    "$gen" --target config      > schemas/config.json
    "$gen" --target openapi     > schemas/openapi.json
    # The annotated config doc (markdown wrapping a fenced YAML block) is
    # derived from `schemas/config.json` and the `#[schemars(example = ...)]`
    # attributes - keep it generated and version-controlled so editors can lean
    # on it as a starting point.
    "$gen" --target config-doc  > docs/content/files/generated_config.md
    # Regenerate `martin/martin-ui/src/lib/types.gen.ts` from the freshly
    # written `schemas/openapi.json`. Kept after the cargo runs so the spec
    # is up-to-date by the time `openapi-typescript` reads it.
    {{just}} ui::gen-ui-types
    martin/martin-ui/node_modules/.bin/biome check --write schemas/config.json schemas/openapi.json

# Validate the generated config + OpenAPI schemas: that they are themselves
# well-formed (against the JSON Schema 2020-12 metaschema and the OpenAPI 3.1
# spec), and that the real config fixtures shipped with martin pass the
# generated config schema. Requires `uv` (provides `uvx`).
test-schemas:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ ! -f schemas/config.json || ! -f schemas/openapi.json ]]; then
        echo "schemas/config.json or schemas/openapi.json missing - run 'just gen-schemas' first" >&2
        exit 1
    fi

    echo "::group::Validate config JSON Schema is itself a valid JSON Schema"
    uvx --from check-jsonschema check-jsonschema \
        --check-metaschema schemas/config.json
    echo "::endgroup::"

    echo "::group::Validate OpenAPI document against the OpenAPI 3.1 spec"
    uvx --from openapi-spec-validator openapi-spec-validator \
        schemas/openapi.json
    echo "::endgroup::"

    echo "::group::Validate real config fixtures against the config schema"
    # The `save_config` snapshots are the post-resolved configs Martin writes via
    # `--save-config`, with no env-substitution placeholders, so they are clean
    # inputs for schema validation. If a real config doesn't validate, the schema
    # is wrong, not the config. Each snapshot carries an insta header ending in a
    # `---` line, which is dropped to leave the config itself.
    fixtures=(
        e2e-tests/tests/snapshots/save_config__every_discovered_source_and_resource_is_spelled_out.snap
        e2e-tests/tests/snapshots/save_config__every_documented_setting_survives_the_round_trip.snap
    )
    for f in "${fixtures[@]}"; do
        if [[ -f "$f" ]]; then
            tmp=$(mktemp --suffix=.yaml)
            awk 'body { print; next } NR > 1 && /^---$/ { body=1 }' "$f" > "$tmp"
            echo "  -> $f (YAML extracted to $tmp)"
            uvx --from check-jsonschema check-jsonschema \
                --schemafile schemas/config.json "$tmp"
            rm -f "$tmp"
        else
            echo "missing $f aborting"
            exit 1
        fi
    done
    echo "::endgroup::"

    # The auto-generated docs example is markdown wrapping a fenced YAML
    # block; extract the YAML and validate it so we catch drift between the
    # schemars derives and the codegen renderer.
    echo "::group::Validate the generated docs example against the config schema"
    doc='docs/content/files/generated_config.md'
    if [[ -f "$doc" ]]; then
        tmp=$(mktemp --suffix=.yaml)
        # Strip everything outside the first ```yaml ... ``` fence.
        awk '
            /^```yaml/  { in_block=1; next }
            /^```/      { if (in_block) { exit } }
            in_block    { print }
        ' "$doc" > "$tmp"
        echo "  -> $doc (YAML extracted to $tmp)"
        uvx --from check-jsonschema check-jsonschema \
            --schemafile schemas/config.json "$tmp"
        rm -f "$tmp"
    else
        echo "missing $doc aborting"
        exit 1
    fi
    echo "::endgroup::"

# Run all tests and save their output as the new expected output (ordering is important)
bless:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Blessing unit tests"
    for target in restart bless-insta ui::bless bless-pg; do
      echo "::group::just $target"
      {{just}} $target
      echo "::endgroup::"
    done

    echo "Blessing end-to-end tests"
    for target in bless-e2e bless-cog bless-duckdb {{ if os() == "linux" { "bless-rendering" } else { "" } }}; do
      echo "::group::just $target"
      {{just}} $target
      echo "::endgroup::"
    done

# Run insta snapshot tests and save their output as the new expected output.
bless-insta *args:  fetch (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    {{insta_test}} --all-targets --workspace {{args}}

# Bless the end-to-end tests, including the ones that need the PostgreSQL database
bless-e2e *args: fetch start (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    cargo build --package martin --package mbtiles
    {{insta_test}} --package martin-e2e-tests --features test-pg {{args}}

bless-pg: fetch start (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    {{insta_test}} --features test-pg --no-default-features --test pg_function_source_test --test pg_reload_test --test pg_server_test --test pg_table_source_test
    {{insta_test}} --features test-pg --no-default-features --package martin --lib
    {{insta_test}} --features test-pg --package martin-core --no-default-features --lib

# Bless the COG/GeoTIFF tests, including the end-to-end ones
bless-cog: fetch (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    {{insta_test}} -p martin --features unstable-cog --no-default-features --lib
    {{insta_test}} -p martin-core --features unstable-cog --no-default-features --lib
    cargo build --package martin --no-default-features --features unstable-cog
    {{insta_test}} --package martin-e2e-tests --features test-cog --test cog

# Bless the DuckDB/GeoParquet tests, including the end-to-end ones
bless-duckdb: fetch (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    {{insta_test}} -p martin -p martin-core --no-default-features --features martin/test-duckdb,martin-core/unstable-duckdb --lib --test duckdb_test
    cargo build -p martin -p martin-core --no-default-features --features martin/test-duckdb,martin-core/unstable-duckdb --bin martin --test duckdb_test
    {{insta_test}} --package martin-e2e-tests --features test-duckdb --test duckdb

# Bless the style rendering tests end-to-end
[linux]
bless-rendering: fetch (cargo-install 'cargo-nextest') (cargo-install 'cargo-insta')
    cargo build --package martin --no-default-features --features rendering
    {{insta_test}} --package martin-e2e-tests --features test-rendering --test rendering

# Build binaries for a target. In release mode (default), strips debug info.
# Set RELEASE_MODE='' to build in debug mode (used for PRs in CI to reduce build time).
build-release target: fetch
    #!/usr/bin/env bash
    set -euo pipefail
    # on debian we need to build a deb package
    if [[ "{{target}}" == "debian-x86_64" ]]; then
        {{just}} build-deb target/debian/debian-x86_64.deb
    else
        rustup target add {{target}}
        if [[ "{{release_mode}}" == "1" ]]; then
            export CARGO_TARGET_{{shoutysnakecase(target)}}_RUSTFLAGS='-C strip=debuginfo'
        fi
        cargo build {{if release_mode == '1' {'--release'} else {''} }} --target {{target}} --package mbtiles --locked
        cargo build {{if release_mode == '1' {'--release'} else {''} }} --target {{target}} --package martin --locked
    fi

# Build debian package
# Note: rendering feature is excluded because the Debian build targets older glibc (ubuntu-22.04)
# and maplibre_native pre-built libraries require newer glibc.
build-deb output: fetch (cargo-install 'cargo-deb')
    sudo apt-get install -y dpkg dpkg-dev liblzma-dev
    cargo deb -v -p martin {{if release_mode == '1' {''} else {'--profile dev'} }} --output {{output}} -- --no-default-features --features {{stable_features}}

# Build for musl target using zigbuild
# Set RELEASE_MODE='' to build in debug mode (used for PRs in CI to reduce build time).
# Note: rendering feature is excluded because maplibre_native cannot be cross-compiled for musl targets.
# -A linker_messages: rustc passes -Wl,-O1 to cc-flavored linkers at opt-level 2+, and zig's linker has no -O levels so it always warns on it.
# Unfixed rustc bug, remove once closed: https://github.com/rust-lang/rust/issues/158192
build-release-musl target: fetch
    rustup target add {{target}}
    {{if release_mode == '1' {'CARGO_TARGET_' + shoutysnakecase(target) + '_RUSTFLAGS="-C strip=debuginfo -A linker_messages"'} else {''} }} cargo zigbuild {{if release_mode == '1' {'--release'} else {''} }} --target {{target}} --package mbtiles --locked
    {{if release_mode == '1' {'CARGO_TARGET_' + shoutysnakecase(target) + '_RUSTFLAGS="-C strip=debuginfo -A linker_messages"'} else {''} }} cargo zigbuild {{if release_mode == '1' {'--release'} else {''} }} --target {{target}} --package martin --locked --no-default-features --features {{stable_features}}


# Move build artifacts to target_releases directory
move-artifacts target:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target_releases
    build_dir={{if release_mode == '1' {'release'} else {'debug'} }}

    if [[ "{{target}}" == "debian-x86_64" ]]; then
        mv target/debian/*.deb target_releases/
    else
        if [[ "{{target}}" == "x86_64-pc-windows-msvc" ]]; then
            mv target/{{target}}/"$build_dir"/martin.exe target_releases/
            mv target/{{target}}/"$build_dir"/martin-cp.exe target_releases/
            mv target/{{target}}/"$build_dir"/mbtiles.exe target_releases/
        else
            mv target/{{target}}/"$build_dir"/martin target_releases/
            mv target/{{target}}/"$build_dir"/martin-cp target_releases/
            mv target/{{target}}/"$build_dir"/mbtiles target_releases/
        fi
    fi


# Quick compile without building a binary. Pass e.g. `--partition 1/4` to run only a subset of the feature matrix
check *args: fetch (cargo-install 'cargo-hack')
    cargo hack --exclude-features _tiles,_catalog,_file_kinds,_process,_raster,_neighbourhood,hotpath,hotpath-alloc,hotpath_tui,unstable-schemas,test-duckdb,test-minio,test-pg check --all-targets --each-feature --workspace --exclude martin-e2e-tests {{args}}

# Verify cargo-binstall metadata resolves correctly
check-binstall: fetch (cargo-install 'cargo-binstall')
    cargo binstall martin --manifest-path martin/Cargo.toml --dry-run --no-confirm

# Test documentation generation
check-doc:  (docs-build)

# Run all tests as expected by CI
ci-test: env-info restart test-fmt clippy check-doc test check && assert-git-is-clean

# Perform  cargo clean  to delete all build files
clean: stop ui::clean
    cargo clean -p static-files
    cargo clean

# Run cargo clippy to lint the code
clippy *args: fetch
    cargo clippy --workspace --all-targets {{args}}

# Validate markdown URLs with markdown-link-check
clippy-md:
    docker run --rm -v ${PWD}:/workdir --entrypoint sh ghcr.io/tcort/markdown-link-check -c \
      'echo -e "/workdir/README.md\n$(find /workdir/docs/content -name "*.md")" | tr "\n" "\0" | xargs -0 -P 5 -n1 -I{} markdown-link-check --config /workdir/.github/files/markdown.links.config.json {}'

# folders ignored from coverage
coverage_ignore_regex := '^e2e-tests/src/'

# Generate code coverage report. Will install `cargo llvm-cov` if missing.
coverage *args='--no-clean --open':  fetch (cargo-install 'cargo-llvm-cov') clean start
    #!/usr/bin/env bash
    set -euo pipefail
    if ! rustup component list | grep llvm-tools-preview > /dev/null; then \
        echo "llvm-tools-preview could not be found. Installing..." ;\
        rustup component add llvm-tools-preview ;\
    fi

    source <(cargo llvm-cov show-env --export-prefix)
    cargo llvm-cov clean --workspace

    echo "::group::Unit tests"
    {{just}} test-cargo --all-targets
    {{just}} test-pg
    echo "::endgroup::"

    # echo "::group::Documentation tests"
    # {{just}} test-doc <- deliberately disabled until --doctest for cargo-llvm-cov does not hang indefinitely
    # echo "::endgroup::"

    {{just}} test-e2e

    cargo llvm-cov report --ignore-filename-regex {{quote(coverage_ignore_regex)}} {{args}}

# Append the cargo-llvm-cov environment to `file` ($GITHUB_ENV), instrumenting every later CI step
coverage-env file: fetch (cargo-install 'cargo-llvm-cov')
    #!/usr/bin/env bash
    set -euo pipefail
    if ! rustup component list --installed | grep llvm-tools > /dev/null; then
        echo "llvm-tools-preview could not be found. Installing..."
        rustup component add llvm-tools-preview
    fi

    cargo llvm-cov clean --profraw-only
    cargo llvm-cov show-env | sed "s/'//g" >> {{quote(file)}}

# Write the coverage recorded since `coverage-env` to `target/lcov-<suite>.info`
coverage-report suite:
    cargo llvm-cov report --lcov --ignore-filename-regex {{quote(coverage_ignore_regex)}} --output-path target/lcov-{{suite}}.info

# Merge the per-suite lcov reports in `dir` into one Cobertura report, unioning their hits
coverage-merge dir='target/coverage' out='target/cobertura.xml': (cargo-install 'grcov')
    grcov {{quote(dir)}} --source-dir . --output-types cobertura --output-path {{quote(out)}}

# Start Martin server
cp *args: fetch
    cargo run --bin martin-cp -- {{args}}

# Start Martin server and open a test page (not the integrated UI)
debug-page *args: start
    open tests/debug.html  # run will not exit, so open debug page first
    {{just}} run {{args}}

# Build and run martin docker image
docker-run *args:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests ghcr.io/maplibre/martin:1.15.0 {{args}}

# Build and run martin documentation
docs:
    uvx zensical serve --open

# Build martin documentation
docs-build:
    docker run --rm -v ${PWD}:/docs zensical/zensical:latest build
# Print environment info
env-info:
    @echo "Running {{if ci_mode == '1' {'in CI mode'} else {'in dev mode'} }} / {{if release_mode == '1' {'release mode'} else {'debug mode'} }} on {{os()}} / {{arch()}}"
    @echo "PWD {{justfile_directory()}}"
    {{just}} --version
    rustc --version
    cargo --version
    @if [ "$(uname)" != "FreeBSD" ]; then rustup --version; fi
    @echo "CARGO_BUILD_WARNINGS='$CARGO_BUILD_WARNINGS'"
    @echo "RUST_BACKTRACE='$RUST_BACKTRACE'"
    npm --version
    node --version

# Reformat all code `cargo fmt`. If nightly is available, use it for better results
fmt: fetch
    #!/usr/bin/env bash
    set -euo pipefail
    if (rustup toolchain list | grep nightly && rustup component list --toolchain nightly | grep rustfmt) &> /dev/null; then
        echo 'Reformatting Rust code using nightly Rust fmt to sort imports'
        cargo +nightly fmt --all -- --config imports_granularity=Module,group_imports=StdExternalCrate
    else
        echo 'Reformatting Rust with the stable cargo fmt.  Install nightly with `rustup install nightly` for better results'
        cargo fmt --all
    fi

# Spellcheck the docs using cspell
spellcheck *args:
    npx --yes cspell@10.1.1 lint --no-progress {{args}}

# Reformat markdown files using markdownlint-cli2
fmt-md:
    docker run --rm -v $PWD:/workdir davidanson/markdownlint-cli2 --config /workdir/.github/files/config.markdownlint-cli2.jsonc --fix

# Reformat all SQL files using docker
fmt-sql:
    docker run -it --rm -v $PWD:/sql sqlfluff/sqlfluff:latest fix --dialect=postgres --exclude-rules=AL07,LT05,LT12 --exclude '^tests/fixtures/(mbtiles|files)/.*\.sql$'
    docker run -it --rm -v $PWD:/sql sqlfluff/sqlfluff:latest fix --dialect=sqlite --exclude-rules=LT01,LT05 --files '^tests/fixtures/(mbtiles|files)/.*\.sql$'

# Reformat all Cargo.toml files using cargo-sort
fmt-toml *args: (cargo-install 'cargo-sort')
    cargo sort --workspace --order package,lib,bin,bench,features,dependencies,build-dependencies,dev-dependencies {{args}}

# Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
[no-exit-message]
git *args: start
    git {{args}}

# Show help for new contributors
help:
    @echo "Common commands:"
    @echo "  just validate-tools    # Check required tools"
    @echo "  just start             # Start test database"
    @echo "  just run               # Start Martin server"
    @echo "  just test              # Run all tests"
    @echo "  just fmt               # Format code"
    @echo "  just docs              # Serve documentation preview"
    @echo ""
    @echo "Full list: just --list"

# Install Linux dependencies (Ubuntu/Debian). Supports 'vulkan' and 'opengl' backends.
[linux]
install-dependencies backend='vulkan':
    sudo apt-get update
    sudo apt-get install -y \
      {{if backend == 'opengl' {'libgl1-mesa-dev libglu1-mesa-dev'} else {''} }} \
      {{if backend == 'vulkan' {'mesa-vulkan-drivers glslang-dev'} else {''} }} \
      build-essential \
      libcurl4-openssl-dev \
      libglfw3-dev \
      libicu-dev \
      libjpeg-dev \
      libpng-dev \
      libuv1-dev \
      libwebp-dev \
      libz-dev

# Install macOS dependencies via Homebrew
[macos]
install-dependencies backend='vulkan':
    brew install \
        {{if backend == 'vulkan' {'molten-vk vulkan-headers'} else {''} }} \
        curl \
        glfw \
        icu4c \
        jpeg-turbo \
        libpng \
        libuv \
        webp \
        zlib

# Install Windows dependencies
[windows]
install-dependencies backend='vulkan':
    @echo "rendering styles is not currently supported on windows"

# Run common lints
lint: fmt check clippy ui::biome ui::type-check clippy-md fmt-toml spellcheck

# Run mbtiles command
mbtiles *args: fetch
    cargo run -p mbtiles -- {{args}}

# Run the fast-mvt `mvt` CLI, e.g. `just mvt dump tile.pbf` to dump a vector tile as readable text
mvt *args: install-mvt
    mvt {{args}}

# Create assets package
package-assets target:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/files
    cd target/{{target}}
    if [[ '{{target}}' == 'x86_64-pc-windows-msvc' ]]; then
        7z a ../files/martin-{{target}}.zip martin.exe martin-cp.exe mbtiles.exe
    elif [[ '{{target}}' == 'debian-x86_64' ]]; then
        mv *.deb ../files/
    else
        chmod +x martin martin-cp mbtiles
        tar czvf ../files/martin-{{target}}.tar.gz martin martin-cp mbtiles
    fi
    cd ../..

# Run pg_dump utility against the test database
pg_dump *args:
    pg_dump {{args}} {{quote(DATABASE_URL)}}

# Update the offline `.sqlx` query cache for the mbtiles crate.
# Build the .mbtiles fixtures from their .sql sources. The tests build their own copies in
# temp directories; this is for serving `tests/fixtures/mbtiles` by hand.
prepare-mbtiles:
    #!/usr/bin/env bash
    set -euo pipefail
    for folder in tests/fixtures/files tests/fixtures/mbtiles; do
        for sql_file in "$folder"/*.sql; do
            mbtiles_file="${sql_file%.sql}.mbtiles"
            echo "Creating $mbtiles_file from $sql_file"
            rm -f "$mbtiles_file"
            sqlite3 "$mbtiles_file" < "$sql_file"
        done
    done

prepare-sqlite: fetch install-sqlx
    #!/usr/bin/env bash
    set -euo pipefail
    db="$(mktemp -t martin-sqlx-verify.XXXXXX)"
    trap 'rm -f "$db"' EXIT
    # add every possible schema to a dummy temp file so that most queries would compile.
    # `init-flat` is listed before the view-defining layouts so `tiles` is a table.
    for f in init-metadata init-flat init-flat-with-hash init-normalized init-normalized-dedup-id init-cache; do
        # it is safer to handle NULLs than to expect it to never be there
        sed -E -e 's/[[:space:]]+NOT[[:space:]]+NULL//g' \
               -e 's/CREATE (TABLE|VIEW) /CREATE \1 IF NOT EXISTS /' \
            "mbtiles/sql/$f.sql" | sqlite3 -bail "$db"
    done
    cd mbtiles
    mkdir -p .sqlx
    cargo sqlx prepare --database-url "sqlite://$db" -- --lib --tests --features transcode
    find .sqlx -name '*.json' -type f -exec sh -c \
      'jq --sort-keys . "$1" > "$1.tmp" && mv "$1.tmp" "$1"' _ {} \;

# Print the connection string for the test database
print-conn-str:
    @echo {{quote(DATABASE_URL)}}

# Run PSQL utility against the test database
psql *args:
    psql {{args}} {{quote(DATABASE_URL)}}

# Restart the test database
restart:
    # sometimes Just optimizes targets, so here we force stop & start by using external just executable
    {{just}} stop
    {{just}} start

# Start Martin server
run *args='--webui enable-for-all': fetch
    cargo run -p martin -- {{args}}

# Start release-compiled Martin server and a test database
run-release *args='--webui enable-for-all': fetch start
    cargo run -p martin --release -- {{args}}

# Check semver compatibility with prior published version. Install it with `cargo install cargo-semver-checks`
semver *args:  fetch (cargo-install 'cargo-semver-checks')
    cargo semver-checks {{args}}

# Start a test database
start:  (docker-up 'db') docker-is-ready

# Start a legacy test database
start-legacy:  (docker-up 'db-legacy') docker-is-ready

# Start an ssl-enabled test database
start-ssl:  (docker-up 'db-ssl') docker-is-ready

# Start an ssl-enabled test database that requires a client certificate
start-ssl-cert:  (docker-up 'db-ssl-cert') docker-is-ready

# Stop the test database
stop:
    docker compose down --remove-orphans

# runs cargo-shear to lint Rust dependencies
shear *args: fetch
    cargo shear {{args}}
    # in the future: add --deny-warnings
    # https://github.com/Boshen/cargo-shear/pull/386

# Run all tests using a test database
test: fetch start
    {{just}} test-cargo --all-targets
    {{just}} test-pg
    {{just}} test-doc
    {{just}} ui::test
    {{just}} test-e2e

# Run PostgreSQL-requiring tests only
test-pg: fetch start (cargo-install 'cargo-nextest')
    cargo nextest run --features test-pg --no-default-features --test pg_function_source_test --test pg_reload_test --test pg_server_test --test pg_table_source_test
    cargo nextest run --features test-pg --no-default-features --package martin --lib
    cargo nextest run --features test-pg --package martin-core --no-default-features --lib
    {{just}} test-e2e-pg

# Run MinIO/S3-requiring tests only (Docker required)
test-minio: fetch (cargo-install 'cargo-nextest')
    cargo nextest run --features test-minio --no-default-features --test pmt_minio_test

# Run COG/GeoTIFF tests only, including the end-to-end ones
test-cog: fetch (cargo-install 'cargo-nextest')
    cargo nextest run -p martin --features unstable-cog --no-default-features --lib
    cargo nextest run -p martin-core --features unstable-cog --no-default-features --lib
    cargo build --package martin --no-default-features --features unstable-cog
    cargo nextest run --package martin-e2e-tests --features test-cog --test cog

# Run DuckDB/GeoParquet tests only, including the end-to-end ones
test-duckdb: fetch (cargo-install 'cargo-nextest')
    cargo nextest run -p martin -p martin-core --no-default-features --features martin/test-duckdb,martin-core/unstable-duckdb --lib --test duckdb_test
    cargo build -p martin -p martin-core --no-default-features --features martin/test-duckdb,martin-core/unstable-duckdb --bin martin --test duckdb_test
    cargo nextest run --package martin-e2e-tests --features test-duckdb --test duckdb

# Run the style rendering tests end-to-end, replaying tests/fixtures/render_cassette
[linux]
test-rendering *args: fetch (cargo-install 'cargo-nextest')
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --package martin --no-default-features --features rendering
    cargo nextest run --package martin-e2e-tests --features test-rendering --test rendering {{args}}

# Run Rust unit tests
test-cargo *args: fetch (cargo-install 'cargo-nextest')
    cargo nextest run {{args}}

# Run unit tests for each package in dependency order
test-packages-ci: fetch (cargo-install 'cargo-nextest')
    #!/usr/bin/env bash
    set -euo pipefail
    cargo nextest run --package martin-tile-utils
    cargo nextest run --package mbtiles --no-default-features
    cargo nextest run --package mbtiles
    cargo nextest run --package martin-core
    cargo nextest run --package martin
    {{just}} test-e2e

# Run the end-to-end tests that drive the compiled martin and mbtiles binaries
test-e2e *args: fetch (cargo-install 'cargo-nextest')
    cargo build --package martin --package mbtiles
    cargo nextest run --package martin-e2e-tests {{args}}

# Run the end-to-end tests that need the PostgreSQL database
test-e2e-pg *args: fetch start (cargo-install 'cargo-nextest')
    cargo build --package martin --package mbtiles
    cargo nextest run --package martin-e2e-tests --features test-pg --test config_file --test martin_cp --test postgres --test process {{args}}

# Run Rust doc tests
test-doc *args: fetch
    cargo test --doc {{args}}

# Test code formatting
test-fmt: fetch (cargo-install 'cargo-sort') && (fmt-toml '--check' '--check-format')
    cargo fmt --all -- --check

# Run AWS Lambda smoke test against SAM local
test-lambda martin_bin='target/debug/martin':
    #!/usr/bin/env bash
    set -euo pipefail

    echo "::group::Build Lambda Function"
    if ! command -v sam >/dev/null 2>&1; then
      echo "The AWS Serverless Application Model Command Line Interface (AWS SAM CLI) is missing."
      echo "  https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html"
      exit 1
    fi
    # `sam build` will copy the _entire_ context to a temporary directory, so just give it the files we need
    mkdir -p .github/files/lambda-layer/bin/
    if ! install {{quote(martin_bin)}} .github/files/lambda-layer/bin/; then
      echo "Specify the binary, e.g. 'just test-lambda target/x86_64-linux-unknown-musl/release/martin'"
      echo "Alternatively, build the binary with 'cargo build -p martin' and it will be used by default"
      exit 1
    fi
    cp ./tests/fixtures/pmtiles2/webp2.pmtiles .github/files/lambda-function/

    # build without touching real credentials
    export AWS_PROFILE=dummy
    export AWS_CONFIG_FILE=.github/files/dummy-aws-config
    sam build --template-file .github/files/lambda.yaml
    echo "::endgroup::"

    # Just send a single request using `sam local invoke` to verify that
    # the server boots, finds a source to serve, and can handle a request.
    # TODO Run the fuller integration suite against this.
    # In doing so, switch from `sam local invoke`, which starts and stops the
    # server, to `sam local start-api`, which keeps it running.
    echo "::group::Generate Event"
    event=$(
      sam local generate-event apigateway http-api-proxy \
        | jq '.rawPath = "/" | .requestContext.http.method = "GET"'
    )
    echo "event:"
    echo "$event" | jq .
    echo "::endgroup::"

    echo "::group::Invoke Lambda Function"
    response=$(sam local invoke -e <(echo "$event"))
    echo "::endgroup::"

    jq -ne 'input.statusCode == 200' <<<"$response"

# Run tests that matter on FreeBSD.
# Notably, we have to skip the postgres tests because the current structure relies on running docker
# within the test. Additionally, some of the benches that run with --all-targets
# are also docker-based integration tests.
# We limit parallelism to prevent OOM during linking of large test binaries.
test-freebsd: (test-cargo "--build-jobs 2 --lib --bins --tests --examples") test-doc

# Run all tests using the oldest supported version of the database
test-legacy: start-legacy (test-cargo "--all-targets") test-pg test-doc

# Run all tests using an SSL connection to a test database
test-ssl: start-ssl (cargo-install 'cargo-nextest') (test-cargo "--all-targets") test-pg test-doc
    cargo build --package martin --package mbtiles

# Install the nextest test runner if not already installed.
[private]
install-nextest:  (cargo-install 'cargo-nextest')
    cargo nextest run --package martin-e2e-tests --features test-pg

# Run all tests using an SSL connection with client cert to a test database
test-ssl-cert: start-ssl-cert (cargo-install 'cargo-nextest')
    #!/usr/bin/env bash
    set -euxo pipefail
    # copy client cert to the tests folder from the docker container
    KEY_DIR=target/certs
    mkdir -p $KEY_DIR
    docker cp martin-db-ssl-cert-1:/etc/ssl/certs/ssl-cert-snakeoil.pem $KEY_DIR/ssl-cert-snakeoil.pem
    docker cp martin-db-ssl-cert-1:/etc/ssl/private/ssl-cert-snakeoil.key $KEY_DIR/ssl-cert-snakeoil.key
    #    export DATABASE_URL="$DATABASE_URL?sslmode=verify-full&sslrootcert=$KEY_DIR/ssl-cert-snakeoil.pem&sslcert=$KEY_DIR/ssl-cert-snakeoil.pem&sslkey=$KEY_DIR/ssl-cert-snakeoil.key"
    export PGSSLROOTCERT="$KEY_DIR/ssl-cert-snakeoil.pem"
    export PGSSLCERT="$KEY_DIR/ssl-cert-snakeoil.pem"
    export PGSSLKEY="$KEY_DIR/ssl-cert-snakeoil.key"
    {{just}} test-cargo --all-targets
    {{just}} test-doc
    cargo build --package martin --package mbtiles
    cargo nextest run --package martin-e2e-tests --features test-pg

# Update all dependencies, including breaking changes. Requires nightly toolchain (install with `rustup install nightly`)
update: fetch
    cargo +nightly -Z unstable-options update --breaking
    # static-files is a direct dep, so reset its manifest cap after --breaking (synced with deny.toml)
    sed 's/^static-files = .*/static-files = "0.2"/' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
    cargo update
    # Make sure that 'evil' dependencies are at the last compatible version
    # below needs to be synced with deny.toml
    cargo update --precise 1.24.0 libdeflater
    cargo update --precise 1.24.0 libdeflate-sys
    cargo update --precise 1.0.2 sdf_glyph_renderer

# Validate that all required development tools are installed
validate-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Validating development tools..."

    # Check essential tools
    missing_tools=()
    if ! command -v jq >/dev/null 2>&1; then
        missing_tools+=("jq")
    fi
    if ! command -v sqlite3 >/dev/null 2>&1; then
        missing_tools+=("sqlite3")
    fi
    # `mvt` dumps a vector tile to a readable form by hand, as `just mvt`. Install it with
    # `just install-mvt` (or `cargo install fast-mvt --features=cli`).
    if ! command -v mvt >/dev/null 2>&1; then
        missing_tools+=("mvt")
    fi

    # Check FreeBSD-specific tools
    if [[ "$OSTYPE" == "freebsd"* ]]; then
        # This should eventually go away if the upstream pbf_glyph_tools can vendor the source artifacts
        # (no more need for protoc). Other platforms automatically install a vendored binary.
        if ! command -v protoc >/dev/null 2>&1; then
            missing_tools+=("protoc")
        fi
    fi

    # Report results
    if [[ ${#missing_tools[@]} -eq 0 ]]; then
        echo "✓ All required tools are installed"
    else
        echo "✗ Missing tools: ${missing_tools[*]}"
        echo "  Ubuntu/Debian: sudo apt install -y jq sqlite3-tools"
        echo "  macOS: brew install jq sqlite"
        echo "  FreeBSD: pkg install jq sqlite3 protobuf"
        echo "  mvt: cargo install fast-mvt --features=cli   (or 'just install-mvt')"
        echo ""
        exit 1
    fi

# Make sure the git repo has no uncommitted changes
[private]
assert-git-is-clean:
    @if [ -n "$(git status --untracked-files --porcelain)" ]; then \
        >&2 echo "ERROR: git repo is no longer clean. Make sure compilation and tests artifacts are in the .gitignore, and no repo files are modified." ;\
        >&2 echo "######### git status ##########" ;\
        git status ;\
        git --no-pager diff ;\
        exit 1 ;\
    fi

# Check if a certain Cargo command is installed, and install it if needed
[private]
cargo-install $COMMAND $INSTALL_CMD='' *args='':
    #!/usr/bin/env bash
    set -euo pipefail
    unset CARGO_BUILD_WARNINGS
    if ! command -v $COMMAND > /dev/null; then
        echo "$COMMAND could not be found. Installing..."
        if ! command -v cargo-binstall > /dev/null; then
            set -x
            cargo install ${INSTALL_CMD:-$COMMAND} --locked {{args}}
            { set +x; } 2>/dev/null
        else
            set -x
            cargo binstall ${INSTALL_CMD:-$COMMAND} {{binstall_args}} --locked
            { set +x; } 2>/dev/null
        fi
    fi

# Fetch all workspace dependencies up front, retrying with exponential backoff to tolerate flaky networks
[private]
fetch:
    #!/usr/bin/env bash
    set -euo pipefail
    delay=5
    for attempt in $(seq 1 5); do
        if cargo fetch --locked; then
            exit 0
        fi
        if [[ "$attempt" -lt 5 ]]; then
            echo "cargo fetch failed (attempt ${attempt}/5); retrying in ${delay}s..." >&2
            sleep "$delay"
            delay=$(( delay * 2 ))
        fi
    done
    echo "cargo fetch failed after 5 attempts" >&2
    exit 1

# Wait for the test database to be ready
[private]
docker-is-ready:
    docker compose run -T --rm db-is-ready

# Start a specific test database, e.g. db or db-legacy
[private]
docker-up name:
    docker compose up -d {{name}}

# Install SQLX cli if not already installed.
[private]
install-sqlx:  (cargo-install 'cargo-sqlx' 'sqlx-cli' '--no-default-features' '--features' 'sqlite,native-tls')

# Install mvt cli if not already installed.
[private]
install-mvt:  (cargo-install 'mvt' 'fast-mvt' '--features=cli')(cargo-install 'cargo-nextest')
