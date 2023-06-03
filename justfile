#!/usr/bin/env just --justfile

set shell := ["bash", "-c"]

export PGPORT := "5411"
export DATABASE_URL := "postgres://postgres:postgres@localhost:" + PGPORT + "/db"
export CARGO_TERM_COLOR := "always"

# export RUST_LOG := "debug"
# export RUST_BACKTRACE := "1"

@_default:
    just --list --unsorted

# Start Martin server and a test database
run *ARGS: start
    cargo run -- {{ ARGS }}

# Start Martin server and open a test page
debug-page *ARGS: start
    open tests/debug.html  # run will not exit, so open debug page first
    just run {{ ARGS }}

# Run PSQL utility against the test database
psql *ARGS:
    psql {{ ARGS }} {{ DATABASE_URL }}

# Perform  cargo clean  to delete all build files
clean: clean-test stop
    cargo clean

# Delete test output files
[private]
clean-test:
    rm -rf tests/output

# Start a test database
start: (docker-up "db")

# Start an ssl-enabled test database
start-ssl: (docker-up "db-ssl")

# Start a legacy test database
start-legacy: (docker-up "db-legacy")

# Start a specific test database, e.g. db or db-legacy
[private]
docker-up name:
    docker-compose up -d {{ name }}
    docker-compose run -T --rm db-is-ready

alias _down := stop
alias _stop-db := stop

# Stop the test database
stop:
    docker-compose down

# Run benchmark tests
bench: start
    cargo bench

# Run all tests using a test database
test: (docker-up "db") test-unit test-int

# Run all tests using an SSL connection to a test database. Expected output won't match.
test-ssl: (docker-up "ssl") test-unit clean-test
    tests/test.sh

# Run all tests using the oldest supported version of the database
test-legacy: (docker-up "db-legacy") test-unit test-int

# Run Rust unit and doc tests (cargo test)
test-unit *ARGS:
    cargo test --all-targets {{ ARGS }}
    cargo test --all-targets --all-features {{ ARGS }}
    cargo test --doc

# Run integration tests
test-int: clean-test
    #!/usr/bin/env bash
    set -euo pipefail
    tests/test.sh
    if ( ! diff --brief --recursive --new-file tests/output tests/expected ); then
        echo "** Expected output does not match actual output"
        echo "** If this is expected, run 'just bless' to update expected output"
        exit 1
    else
        echo "Expected output matches actual output"
    fi

# Run integration tests and save its output as the new expected output
bless: start clean-test
    tests/test.sh
    rm -rf tests/expected
    mv tests/output tests/expected

# Build and open mdbook documentation
mdbook:
    @if ! command -v mdbook &> /dev/null; then \
        echo "mdbook could not be found. Installing..." ;\
        cargo install mdbook ;\
    fi
    mdbook serve docs --open

# Build and open code documentation
docs:
    cargo doc --no-deps --open

# Run code coverage on tests and save its output in the coverage directory. Parameter could be html or lcov.
coverage FORMAT='html':
    #!/usr/bin/env bash
    set -euo pipefail
    @if ! command -v grcov &> /dev/null; then \
        echo "grcov could not be found. Installing..." ;\
        cargo install grcov ;\
    fi
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
    cargo test --all-targets --all-features
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

# Run cargo clippy
clippy:
    cargo clippy --workspace --all-targets --all-features --bins --tests --lib --benches -- -D warnings

# These steps automatically run before git push via a git hook
[private]
git-pre-push: stop start
    rustc --version
    cargo --version
    just lint
    just test

# Update sqlite database schema. Install SQLX cli if not already installed.
prepare-sqlite:
    @if ! command -v cargo-sqlx &> /dev/null; then \
        echo "SQLX could not be found. Installing..." ;\
        cargo install sqlx-cli --no-default-features --features sqlite,native-tls ;\
    fi
    cd martin-mbtiles && cargo sqlx prepare --database-url sqlite://$PWD/../tests/fixtures/files/world_cities.mbtiles
