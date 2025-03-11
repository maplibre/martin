#!/usr/bin/env just --justfile

set shell := ["bash", "-c"]

#export DATABASE_URL="postgres://postgres:postgres@localhost:5411/db"

# Set additional database connection parameters, e.g.   just  PGPARAMS='keepalives=0&keepalives_idle=15'  psql
PGPARAMS := ""
PGPORT := "5411"

export DATABASE_URL := "postgres://postgres:postgres@localhost:" + PGPORT + "/db" + (if PGPARAMS != "" { "?" + PGPARAMS } else { "" })
export CARGO_TERM_COLOR := "always"

#export RUST_LOG := "debug"
#export RUST_LOG := "sqlx::query=info,trace"
#export RUST_BACKTRACE := "1"

dockercompose := `if docker-compose --version &> /dev/null; then echo "docker-compose"; else echo "docker compose"; fi`

@_default:
    {{just_executable()}} --list --unsorted

# Start Martin server
run *ARGS="--webui enable-for-all":
    cargo run -p martin -- {{ARGS}}

# Start Martin server
cp *ARGS:
    cargo run --bin martin-cp -- {{ARGS}}

# Run mbtiles command
mbtiles *ARGS:
    cargo run -p mbtiles -- {{ARGS}}

# Start release-compiled Martin server and a test database
run-release *ARGS="--webui enable-for-all": start
    cargo run -p martin --release -- {{ARGS}}

# Start Martin server and open a test page
debug-page *ARGS: start
    open tests/debug.html  # run will not exit, so open debug page first
    {{just_executable()}} run {{ARGS}}

# Run PSQL utility against the test database
psql *ARGS:
    psql {{ARGS}} {{quote(DATABASE_URL)}}

# Run pg_dump utility against the test database
pg_dump *ARGS:
    pg_dump {{ARGS}} {{quote(DATABASE_URL)}}

# Perform  cargo clean  to delete all build files
clean: clean-test stop && clean-martin-ui
    cargo clean

clean-martin-ui:
    rm -rf martin/martin-ui/dist martin/martin-ui/node_modules
    cargo clean -p static-files

# Delete test output files
[private]
clean-test:
    rm -rf tests/output

# Start a test database
start: (docker-up "db") docker-is-ready

# Start an ssl-enabled test database
start-ssl: (docker-up "db-ssl") docker-is-ready

# Start an ssl-enabled test database that requires a client certificate
start-ssl-cert: (docker-up "db-ssl-cert") docker-is-ready

# Start a legacy test database
start-legacy: (docker-up "db-legacy") docker-is-ready

# Start a specific test database, e.g. db or db-legacy
[private]
docker-up name: start-pmtiles-server
    {{dockercompose}} up -d {{name}}

# Wait for the test database to be ready
[private]
docker-is-ready:
    {{dockercompose}} run -T --rm db-is-ready

alias _down := stop
alias _stop-db := stop

# Restart the test database
restart:
    # sometimes Just optimizes targets, so here we force stop & start by using external just executable
    {{just_executable()}} stop
    {{just_executable()}} start

# Stop the test database
stop:
    {{dockercompose}} down --remove-orphans

# Start test server for testing HTTP pmtiles
start-pmtiles-server:
    {{dockercompose}} up -d fileserver

# Run benchmark tests
bench:
    cargo bench --bench bench
    open target/criterion/report/index.html

# Run benchmark tests showing a flamegraph
flamegraph:
    cargo bench --bench bench -- --profile-time=10
    /opt/google/chrome/chrome "file://$PWD/target/criterion/get_table_source_tile/profile/flamegraph.svg"

# Start release-compiled Martin server and a test database
bench-server: start
    cargo run --release -- tests/fixtures/mbtiles tests/fixtures/pmtiles

# Run HTTP requests benchmark using OHA tool. Use with `just bench-server`
bench-http: (cargo-install "oha")
    @echo "ATTENTION: Make sure Martin was started with    just bench-server"
    @echo "Warming up..."
    oha --latency-correction -z 5s --no-tui http://localhost:3000/function_zxy_query/18/235085/122323 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/function_zxy_query/18/235085/122323
    oha --latency-correction -z 5s --no-tui http://localhost:3000/png/0/0/0 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/png/0/0/0
    oha --latency-correction -z 5s --no-tui http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0

# Run all tests using a test database
test: start (test-cargo "--all-targets") test-doc test-int

# Run all tests using an SSL connection to a test database. Expected output won't match.
test-ssl: start-ssl (test-cargo "--all-targets") test-doc clean-test
    tests/test.sh

# Run all tests using an SSL connection with client cert to a test database. Expected output won't match.
test-ssl-cert: start-ssl-cert
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
    {{just_executable()}} test-cargo --all-targets
    {{just_executable()}} clean-test
    {{just_executable()}} test-doc
    tests/test.sh

# Run all tests using the oldest supported version of the database
test-legacy: start-legacy (test-cargo "--all-targets") test-doc test-int

# Run Rust unit tests (cargo test)
test-cargo *ARGS:
    cargo test {{ARGS}}

# Run Rust doc tests
test-doc *ARGS:
    cargo test --doc {{ARGS}}

# Run integration tests
test-int: clean-test install-sqlx
    #!/usr/bin/env bash
    set -euo pipefail
    tests/test.sh
    if [ "{{os()}}" != "linux" ]; then
        echo "** Integration tests are only supported on Linux"
        echo "** Skipping diffing with the expected output"
    else
        echo "** Comparing actual output with expected output..."
        if ! diff --brief --recursive --new-file --exclude='*.pbf' tests/output tests/expected; then
            echo "** Expected output does not match actual output"
            echo "** If this is expected, run 'just bless' to update expected output"
            exit 1
        else
            echo "** Expected output matches actual output"
        fi
    fi

# Run AWS Lambda smoke test against SAM local
test-lambda:
    tests/test-aws-lambda.sh

# Run integration tests and save its output as the new expected output (ordering is important, but in some cases run `bless-tests` before others)
bless: restart clean-test bless-insta-martin bless-insta-mbtiles bless-tests bless-int

# Bless integration tests
bless-int:
    rm -rf tests/temp
    tests/test.sh
    rm -rf tests/expected && mv tests/output tests/expected

# Run test with bless-tests feature
bless-tests:
    cargo test -p martin --features bless-tests

# Run integration tests and save its output as the new expected output
bless-insta-mbtiles *ARGS: (cargo-install "cargo-insta")
    #rm -rf mbtiles/tests/snapshots
    cargo insta test --accept --unreferenced=auto -p mbtiles {{ARGS}}

# Run integration tests and save its output as the new expected output
bless-insta-martin *ARGS: (cargo-install "cargo-insta")
    cargo insta test --accept --unreferenced=auto -p martin {{ARGS}}

# Run integration tests and save its output as the new expected output
bless-insta-cp *ARGS: (cargo-install "cargo-insta")
    cargo insta test --accept --bin martin-cp {{ARGS}}

# Build and open mdbook documentation
book: (cargo-install "mdbook")
    mdbook serve docs --open --port 8321

# Build debian package
package-deb: (cargo-install "cargo-deb")
    cargo deb -v -p martin --output target/debian/martin.deb

# Build and open code documentation
docs:
    cargo doc --no-deps --open

# Run code coverage on tests and save its output in the coverage directory. Parameter could be html or lcov.
coverage FORMAT='html': (cargo-install "grcov")
    #!/usr/bin/env bash
    set -euo pipefail
    if ! rustup component list | grep llvm-tools-preview > /dev/null; then \
        echo "llvm-tools-preview could not be found. Installing..." ;\
        rustup component add llvm-tools-preview ;\
    fi

    {{just_executable()}} clean
    {{just_executable()}} start

    PROF_DIR=target/prof
    mkdir -p "$PROF_DIR"
    PROF_DIR=$(realpath "$PROF_DIR")

    OUTPUT_RESULTS_DIR=target/coverage/{{FORMAT}}
    mkdir -p "$OUTPUT_RESULTS_DIR"

    export CARGO_INCREMENTAL=0
    export RUSTFLAGS=-Cinstrument-coverage
    # Avoid problems with relative paths
    export LLVM_PROFILE_FILE=$PROF_DIR/cargo-test-%p-%m.profraw
    export MARTIN_PORT=3111

    cargo test --all-targets
    tests/test.sh

    set -x
    grcov --binary-path ./target/debug    \
          -s .                            \
          -t {{FORMAT}}                 \
          --branch                        \
          --ignore 'benches/*'            \
          --ignore 'tests/*'              \
          --ignore-not-existing           \
          -o target/coverage/{{FORMAT}} \
          --llvm                          \
          "$PROF_DIR"
    { set +x; } 2>/dev/null

    # if this is html, open it in the browser
    if [ "{{FORMAT}}" = "html" ]; then
        open "$OUTPUT_RESULTS_DIR/index.html"
    fi

# Build and run martin docker image
docker-run *ARGS:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests ghcr.io/maplibre/martin {{ARGS}}

# Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
[no-exit-message]
git *ARGS: start
    git {{ARGS}}

# Print the connection string for the test database
print-conn-str:
    @echo {{quote(DATABASE_URL)}}

# Run cargo fmt and cargo clippy
lint: fmt clippy

# Run cargo fmt
fmt:
    cargo fmt --all -- --check

# Reformat markdown files using markdownlint-cli2
fmt-md:
    docker run -it --rm -v $PWD:/workdir davidanson/markdownlint-cli2 --config /workdir/.github/files/config.markdownlint-cli2.jsonc --fix

# Run Nightly cargo fmt, ordering imports
fmt2:
    cargo +nightly fmt -- --config imports_granularity=Module,group_imports=StdExternalCrate

# Run cargo check
check:
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin-tile-utils
    RUSTFLAGS='-D warnings' cargo check --all-targets -p mbtiles
    RUSTFLAGS='-D warnings' cargo check --all-targets -p mbtiles --no-default-features
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features --features fonts
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features --features mbtiles
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features --features pmtiles
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features --features postgres
    RUSTFLAGS='-D warnings' cargo check --all-targets -p martin --no-default-features --features sprites

# Verify doc build
check-doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

# Run cargo clippy
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Validate markdown URLs with markdown-link-check
clippy-md:
    docker run -it --rm -v ${PWD}:/workdir --entrypoint sh ghcr.io/tcort/markdown-link-check -c \
      'echo -e "/workdir/README.md\n$(find /workdir/docs/src -name "*.md")" | tr "\n" "\0" | xargs -0 -P 5 -n1 -I{} markdown-link-check --config /workdir/.github/files/markdown.links.config.json {}'

# Update dependencies, including breaking changes
update:
    cargo +nightly -Z unstable-options update --breaking
    cargo update

# A few useful tests to run locally to simulate CI
ci-test: env-info restart fmt clippy check-doc test check

# These steps automatically run before git push via a git hook
git-pre-push:
    # TODO: these should be deleted after a while
    echo "Pre-commit is no longer required."
    echo "Please remove the git hook by running    rm .git/hooks/pre-push"
    exit 1

# Get environment info
[private]
env-info:
    @echo "OS is {{os()}}, arch is {{arch()}}"
    {{just_executable()}} --version
    rustc --version
    cargo --version
    rustup --version

# Update sqlite database schema.
prepare-sqlite: install-sqlx
    mkdir -p mbtiles/.sqlx
    cd mbtiles && cargo sqlx prepare --database-url sqlite://$PWD/../tests/fixtures/mbtiles/world_cities.mbtiles -- --lib --tests

# Install SQLX cli if not already installed.
[private]
install-sqlx: (cargo-install "cargo-sqlx" "sqlx-cli" "--no-default-features" "--features" "sqlite,native-tls")

# Check if a certain Cargo command is installed, and install it if needed
[private]
cargo-install $COMMAND $INSTALL_CMD="" *ARGS="":
    #!/usr/bin/env sh
    set -eu
    if ! command -v $COMMAND > /dev/null; then
        if ! command -v cargo-binstall > /dev/null; then
            echo "$COMMAND could not be found. Installing it with    cargo install ${INSTALL_CMD:-$COMMAND} --locked {{ARGS}}"
            cargo install ${INSTALL_CMD:-$COMMAND} --locked {{ARGS}}
        else
            echo "$COMMAND could not be found. Installing it with    cargo binstall ${INSTALL_CMD:-$COMMAND} --locked {{ARGS}}"
            cargo binstall ${INSTALL_CMD:-$COMMAND} --locked {{ARGS}}
        fi
    fi
