#!/usr/bin/env just --justfile
set shell := ["bash", "-c"]

export DATABASE_URL := "postgres://postgres@localhost/db"
export CARGO_TERM_COLOR := "always"
# export RUST_LOG := "debug"
# export RUST_BACKTRACE := "1"

@_default:
  just --list --unsorted

# Start Martin server and a test database
run *ARGS: start-db
    cargo run -- {{ARGS}}

# Start Martin server and open a test page
debug-page *ARGS: start-db
    open tests/debug.html  # run will not exit, so open debug page first
    just run {{ARGS}}

# Run PSQL utility against the test database
psql *ARGS: start-db
    psql {{ARGS}} {{DATABASE_URL}}

# Perform  cargo clean  to delete all build files
clean: clean-test stop
    cargo clean

# Delete test output files
clean-test:
    rm -rf tests/output

# Start a test database
start-db: (docker-up "db")

# Start a legacy test database
start-legacy: (docker-up "db-legacy")

# Start a specific test database, e.g. db or db-legacy
@docker-up name:
    docker-compose up -d {{name}}

alias _down := stop
alias _stop-db := stop

# Stop the test database
stop:
    docker-compose down

# Run benchmark tests
bench: start-db
    cargo bench

# Run all tests using a test database
test: test-unit test-int

# Run Rust unit and doc tests (cargo test)
test-unit *ARGS: start-db
    cargo test --all-targets {{ARGS}}
    cargo test --all-targets --all-features {{ARGS}}
    cargo test --doc

# Run integration tests
test-int: (test-integration "db")

# Run integration tests using legacy database
test-int-legacy: (test-integration "db-legacy")

# Run integration tests with the given docker compose target
@test-integration name: (docker-up name) clean-test
    #!/usr/bin/env sh
    export MARTIN_PORT=3111
    tests/test.sh
# echo "** Skipping comparison with the expected values - not yet stable"
# if ( ! diff --brief --recursive --new-file tests/output tests/expected ); then
#     echo "** Expected output does not match actual output"
#     echo "** If this is expected, run 'just bless' to update expected output"
#     echo "** Note that this error is not fatal because we don't have a stable output yet"
# fi

# # Run integration tests and save its output as the new expected output
# bless: start-db clean-test
#     tests/test.sh
#     rm -rf tests/expected
#     mv tests/output tests/expected

# Build martin docker image
docker-build:
    docker build -t martin .

# Build and run martin docker image
docker-run *ARGS:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests martin {{ARGS}}

# Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
[no-exit-message]
git *ARGS: start-db
    git {{ARGS}}

# Run cargo fmt and cargo clippy
lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings -W clippy::pedantic

# These steps automatically run before git push via a git hook
git-pre-push: stop start-db
    rustc --version
    cargo --version
    just lint
    just test
