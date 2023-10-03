#!/usr/bin/env just --justfile

set shell := ["bash", "-c"]

export PGPORT := "5411"
export DATABASE_URL := "postgres://postgres:postgres@localhost:" + PGPORT + "/db"
export CARGO_TERM_COLOR := "always"

#export RUST_LOG := "debug"
#export RUST_LOG := "sqlx::query=info,trace"
#export RUST_BACKTRACE := "1"

@_default:
    just --list --unsorted

# Start Martin server
run *ARGS:
    cargo run -- {{ ARGS }}

# Start release-compiled Martin server and a test database
run-release *ARGS: start
    cargo run -- {{ ARGS }}

# Start Martin server and open a test page
debug-page *ARGS: start
    open tests/debug.html  # run will not exit, so open debug page first
    just run {{ ARGS }}

# Run PSQL utility against the test database
psql *ARGS:
    psql {{ ARGS }} {{ DATABASE_URL }}

# Run pg_dump utility against the test database
pg_dump *ARGS:
    pg_dump {{ ARGS }} {{ DATABASE_URL }}

# Perform  cargo clean  to delete all build files
clean: clean-test stop
    cargo clean

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
docker-up name:
    docker-compose up -d {{ name }}

# Wait for the test database to be ready
[private]
docker-is-ready:
    docker-compose run -T --rm db-is-ready

alias _down := stop
alias _stop-db := stop

# Restart the test database
restart:
    just stop
    just start

# Stop the test database
stop:
    docker-compose down

# Run benchmark tests
bench: start
    cargo bench

# Run HTTP requests benchmark using OHA tool. Use with `just run-release`
bench-http: (cargo-install "oha")
    @echo "Make sure Martin was started with 'just run-release'"
    @echo "Warming up..."
    oha -z 5s --no-tui http://localhost:3000/function_zxy_query/18/235085/122323 > /dev/null
    oha -z 120s  http://localhost:3000/function_zxy_query/18/235085/122323

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
    cargo test {{ ARGS }}

# Run Rust doc tests
test-doc *ARGS:
    cargo test --doc {{ ARGS }}

# Run integration tests
test-int: clean-test install-sqlx
    #!/usr/bin/env bash
    set -euo pipefail
    tests/test.sh
    if ! diff --brief --recursive --new-file tests/output tests/expected; then
        echo "** Expected output does not match actual output"
        echo "** If this is expected, run 'just bless' to update expected output"
        exit 1
    else
        echo "Expected output matches actual output"
    fi

# Run integration tests and save its output as the new expected output
bless: start clean-test bless-insta
    rm -rf tests/temp
    cargo test -p martin --features bless-tests
    tests/test.sh
    rm -rf tests/expected
    mv tests/output tests/expected

bless-insta *ARGS: (cargo-install "insta" "cargo-insta")
    #rm -rf martin-mbtiles/tests/snapshots
    cargo insta test --accept --unreferenced=auto -p martin-mbtiles {{ ARGS }}

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
    if ! rustup component list | grep llvm-tools-preview &> /dev/null; then \
        echo "llvm-tools-preview could not be found. Installing..." ;\
        rustup component add llvm-tools-preview ;\
    fi

    just clean
    just start

    PROF_DIR=target/prof
    mkdir -p "$PROF_DIR"
    PROF_DIR=$(realpath "$PROF_DIR")

    OUTPUT_RESULTS_DIR=target/coverage/{{ FORMAT }}
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
          -t {{ FORMAT }}                 \
          --branch                        \
          --ignore 'benches/*'            \
          --ignore 'tests/*'              \
          --ignore-not-existing           \
          -o target/coverage/{{ FORMAT }} \
          --llvm                          \
          "$PROF_DIR"
    { set +x; } 2>/dev/null

    # if this is html, open it in the browser
    if [ "{{ FORMAT }}" = "html" ]; then
        open "$OUTPUT_RESULTS_DIR/index.html"
    fi

# Build martin docker image
docker-build:
    docker build -t ghcr.io/maplibre/martin .

# Build and run martin docker image
docker-run *ARGS:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests ghcr.io/maplibre/martin {{ ARGS }}

# Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
[no-exit-message]
git *ARGS: start
    git {{ ARGS }}

# Print the connection string for the test database
print-conn-str:
    @echo {{ DATABASE_URL }}

# Run cargo fmt and cargo clippy
lint: fmt clippy

# Run cargo fmt
fmt:
    cargo fmt --all -- --check

# Run Nightly cargo fmt, ordering imports
fmt2:
    cargo +nightly fmt -- --config imports_granularity=Module,group_imports=StdExternalCrate

# Run cargo clippy
clippy:
    cargo clippy --workspace --all-targets --bins --tests --lib --benches -- -D warnings
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

# These steps automatically run before git push via a git hook
[private]
git-pre-push: stop start
    rustc --version
    cargo --version
    just lint
    just test

# Update sqlite database schema.
prepare-sqlite: install-sqlx
    mkdir -p martin-mbtiles/.sqlx
    cd martin-mbtiles && cargo sqlx prepare --database-url sqlite://$PWD/../tests/fixtures/mbtiles/world_cities.mbtiles -- --lib --tests

# Install SQLX cli if not already installed.
[private]
install-sqlx: (cargo-install "cargo-sqlx" "sqlx-cli" "--no-default-features" "--features" "sqlite,native-tls")

# Check if a certain Cargo command is installed, and install it if needed
[private]
cargo-install $COMMAND $INSTALL_CMD="" *ARGS="":
    @if ! command -v $COMMAND &> /dev/null; then \
        echo "$COMMAND could not be found. Installing it with    cargo install ${INSTALL_CMD:-$COMMAND} {{ ARGS }}" ;\
        cargo install ${INSTALL_CMD:-$COMMAND} {{ ARGS }} ;\
    fi
