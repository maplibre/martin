#!/usr/bin/env just --justfile
set shell := ["bash", "-c"]

export DATABASE_URL := "postgres://postgres@localhost/db"
export CARGO_TERM_COLOR := "always"
# export RUST_BACKTRACE := "1"

@_default:
  just --list

# Start Martin server and a test database
run: start-db
    cargo run

# Perform  cargo clean  to delete all build files
clean: clean-test
    cargo clean

# Delete test output files
clean-test:
    rm -rf tests/output

# Start a test database
start-db:
    docker-compose up -d db

alias _down := stop
alias _stop-db := stop

# Stop the test database
stop:
    docker-compose down

# Run benchmark tests
bench: start-db
    cargo bench

# Run all tests using a test database
test: start-db test-unit test-int

# Run Rust unit tests (cargo test)
test-unit: start-db
    cargo test

# Run integration tests
test-int: start-db clean-test
    tests/test.sh
    diff --brief --recursive --new-file tests/output tests/expected

# Run integration tests and save its output as the new expected output
test-bless: start-db clean-test
    tests/test.sh
    rm -rf tests/expected
    mv tests/output tests/expected
