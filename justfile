#!/usr/bin/env just --justfile

set shell := ['bash', '-c']


main_crate := 'martin'

#export DATABASE_URL='postgres://postgres:postgres@localhost:5411/db'

# Set additional database connection parameters, e.g.   just  PGPARAMS='keepalives=0&keepalives_idle=15'  psql
PGPARAMS := ''
PGPORT := '5411'

export DATABASE_URL := ('postgres://postgres:postgres@localhost:' + PGPORT + '/db' + (if PGPARAMS != '' { '?' + PGPARAMS } else { '' }))
export CARGO_TERM_COLOR := 'always'

# Set AWS variables for testing pmtiles from S3
export AWS_SKIP_CREDENTIALS := '1'
export AWS_REGION := 'eu-central-1'

#export RUST_LOG := 'debug'
#export RUST_LOG := 'sqlx::query=info,trace'
#export RUST_BACKTRACE := '1'

dockercompose := `if docker-compose --version &> /dev/null; then echo "docker-compose"; else echo "docker compose"; fi`

# if running in CI, treat warnings as errors by setting RUSTFLAGS and RUSTDOCFLAGS to '-D warnings' unless they are already set
# Use `CI=true just ci-test` to run the same tests as in GitHub CI.
# Use `just env-info` to see the current values of RUSTFLAGS and RUSTDOCFLAGS
ci_mode := if env('CI', '') != '' {'1'} else {''}
# cargo-binstall needs a workaround due to caching
# ci_mode might be manually set by user, so re-check the env var
binstall_args := if env('CI', '') != '' {'--no-track'} else {''}
export RUSTFLAGS := env('RUSTFLAGS', if ci_mode == '1' {'-D warnings'} else {''})
export RUSTDOCFLAGS := env('RUSTDOCFLAGS', if ci_mode == '1' {'-D warnings'} else {''})
export RUST_BACKTRACE := env('RUST_BACKTRACE', if ci_mode == '1' {'1'} else {''})

@_default:
    {{just_executable()}} --list

# Run benchmark tests
bench:
    cargo bench --bench bench
    open target/criterion/report/index.html

# Run HTTP requests benchmark using OHA tool. Use with `just bench-server`
bench-http:  (cargo-install 'oha')
    @echo "ATTENTION: Make sure Martin was started with    just bench-server"
    @echo "Warming up..."
    oha --latency-correction -z 5s --no-tui http://localhost:3000/function_zxy_query/18/235085/122323 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/function_zxy_query/18/235085/122323
    oha --latency-correction -z 5s --no-tui http://localhost:3000/png/0/0/0 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/png/0/0/0
    oha --latency-correction -z 5s --no-tui http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0 > /dev/null
    oha --latency-correction -z 60s         http://localhost:3000/stamen_toner__raster_CC-BY-ODbL_z3/0/0/0

# Start release-compiled Martin server and a test database
bench-server: start
    cargo run --release -- tests/fixtures/mbtiles tests/fixtures/pmtiles

# Run biomejs on the dashboard (martin/martin-ui)
[working-directory: 'martin/martin-ui']
biomejs-martin-ui:
    npm run format
    npm run lint

# Run integration tests and save its output as the new expected output (ordering is important)
bless: restart clean-test bless-insta-martin bless-insta-mbtiles bless-frontend bless-int

# Bless the frontend tests
[working-directory: 'martin/martin-ui']
bless-frontend:
    npm run test:update-snapshots

# Run integration tests and save its output as the new expected output
bless-insta-cp *args:  (cargo-install 'cargo-insta')
    cargo insta test --accept --bin martin-cp {{args}}

# Run integration tests and save its output as the new expected output
bless-insta-martin *args:  (cargo-install 'cargo-insta')
    cargo insta test --accept -p martin {{args}}

# Run integration tests and save its output as the new expected output
bless-insta-mbtiles *args:  (cargo-install 'cargo-insta')
    #rm -rf mbtiles/tests/snapshots
    cargo insta test --accept -p mbtiles {{args}}

# Bless integration tests
bless-int:
    rm -rf tests/temp
    tests/test.sh
    rm -rf tests/expected && mv tests/output tests/expected

# Build and open mdbook documentation
book:  (cargo-install 'mdbook') (cargo-install 'mdbook-alerts')
    mdbook serve docs --open --port 8321

# Quick compile without building a binary
check:
    cargo check --all-targets -p martin-tile-utils
    cargo check --all-targets -p mbtiles
    cargo check --all-targets -p mbtiles --no-default-features
    cargo check --all-targets -p martin
    cargo check --all-targets -p martin --no-default-features
    for feature in $({{just_executable()}} get-features); do \
        echo "Checking '$feature' feature" >&2 ;\
        cargo check --all-targets -p martin --no-default-features --features $feature ;\
    done

# Test documentation generation
check-doc:  (docs '')

# Run all tests as expected by CI
ci-test: env-info restart test-fmt clippy check-doc test check && assert-git-is-clean

# Perform  cargo clean  to delete all build files
clean: clean-test stop && clean-martin-ui
    cargo clean

clean-martin-ui:
    rm -rf martin/martin-ui/dist martin/martin-ui/node_modules
    cargo clean -p static-files

# Run cargo clippy to lint the code
clippy *args:
    cargo clippy --workspace --all-targets {{args}}

# Validate markdown URLs with markdown-link-check
clippy-md:
    docker run -it --rm -v ${PWD}:/workdir --entrypoint sh ghcr.io/tcort/markdown-link-check -c \
      'echo -e "/workdir/README.md\n$(find /workdir/docs/src -name "*.md")" | tr "\n" "\0" | xargs -0 -P 5 -n1 -I{} markdown-link-check --config /workdir/.github/files/markdown.links.config.json {}'

# Generate code coverage report. Will install `cargo llvm-cov` if missing.
coverage *args='--no-clean --open':  (cargo-install 'cargo-llvm-cov') clean start
    #!/usr/bin/env bash
    set -euo pipefail
    if ! rustup component list | grep llvm-tools-preview > /dev/null; then \
        echo "llvm-tools-preview could not be found. Installing..." ;\
        rustup component add llvm-tools-preview ;\
    fi

    source <(cargo llvm-cov show-env --export-prefix)
    cargo llvm-cov clean --workspace

    {{just_executable()}} test-cargo --all-targets
    # {{just_executable()}} test-doc <- deliberately disabled until --doctest for cargo-llvm-cov does not hang indefinitely
    {{just_executable()}} test-int

    cargo llvm-cov report {{args}}

# Start Martin server
cp *args:
    cargo run --bin martin-cp -- {{args}}

# Build and run martin docker image
docker-run *args:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests ghcr.io/maplibre/martin {{args}}

# Build and open code documentation
docs *args='--open':
    DOCS_RS=1 cargo doc --no-deps {{args}} --workspace

# Print environment info
env-info:
    @echo "Running {{if ci_mode == '1' {'in CI mode'} else {'in dev mode'} }} on {{os()}} / {{arch()}}"
    @echo "PWD $(pwd)"
    {{just_executable()}} --version
    rustc --version
    cargo --version
    rustup --version
    @echo "RUSTFLAGS='$RUSTFLAGS'"
    @echo "RUSTDOCFLAGS='$RUSTDOCFLAGS'"
    @echo "RUST_BACKTRACE='$RUST_BACKTRACE'"
    npm --version
    node --version

# Run benchmark tests showing a flamegraph
flamegraph:
    cargo bench --bench bench -- --profile-time=10
    /opt/google/chrome/chrome "file://$PWD/target/criterion/get_table_source_tile/profile/flamegraph.svg"

# Reformat all code `cargo fmt`. If nightly is available, use it for better results
fmt:
    #!/usr/bin/env bash
    set -euo pipefail
    if (rustup toolchain list | grep nightly && rustup component list --toolchain nightly | grep rustfmt) &> /dev/null; then
        echo 'Reformatting Rust code using nightly Rust fmt to sort imports'
        cargo +nightly fmt --all -- --config imports_granularity=Module,group_imports=StdExternalCrate
    else
        echo 'Reformatting Rust with the stable cargo fmt.  Install nightly with `rustup install nightly` for better results'
        cargo fmt --all
    fi

# Reformat markdown files using markdownlint-cli2
fmt-md:
    docker run -it --rm -v $PWD:/workdir davidanson/markdownlint-cli2 --config /workdir/.github/files/config.markdownlint-cli2.jsonc --fix

# Reformat all SQL files using docker
fmt-sql:
    docker run -it --rm -v $PWD:/sql sqlfluff/sqlfluff:latest fix --dialect=postgres --exclude-rules=AL07,LT05,LT12

# Reformat all Cargo.toml files using cargo-sort
fmt-toml *args: (cargo-install 'cargo-sort')
    cargo sort --workspace --order package,lib,bin,bench,features,dependencies,build-dependencies,dev-dependencies {{args}}

# Get all testable features of the main crate as space-separated list
get-features:
    cargo metadata --format-version=1 --no-deps --manifest-path Cargo.toml | jq -r '.packages[] | select(.name == "{{main_crate}}") | .features | keys[] | select(. != "default")' | tr '\n' ' '

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
    @echo "  just book              # Build documentation"
    @echo ""
    @echo "Full list: just --list"

# Run cargo fmt and cargo clippy
lint: fmt clippy biomejs-martin-ui type-check

# Run mbtiles command
mbtiles *args:
    cargo run -p mbtiles -- {{args}}

# Build debian package
package-deb:  (cargo-install 'cargo-deb')
    cargo deb -v -p martin --output target/debian/martin.deb

# Run pg_dump utility against the test database
pg_dump *args:
    pg_dump {{args}} {{quote(DATABASE_URL)}}

# Update sqlite database schema.
prepare-sqlite: install-sqlx
    mkdir -p mbtiles/.sqlx
    cd mbtiles && cargo sqlx prepare --database-url sqlite://$PWD/../tests/fixtures/mbtiles/world_cities.mbtiles -- --lib --tests

# Print the connection string for the test database
print-conn-str:
    @echo {{quote(DATABASE_URL)}}

# Run PSQL utility against the test database
psql *args:
    psql {{args}} {{quote(DATABASE_URL)}}

# Restart the test database
restart:
    # sometimes Just optimizes targets, so here we force stop & start by using external just executable
    {{just_executable()}} stop
    {{just_executable()}} start

# Start Martin server
run *args='--webui enable-for-all':
    cargo run -p martin -- {{args}}

# Start release-compiled Martin server and a test database
run-release *args='--webui enable-for-all': start
    cargo run -p martin --release -- {{args}}

# Check semver compatibility with prior published version. Install it with `cargo install cargo-semver-checks`
semver *args:  (cargo-install 'cargo-semver-checks')
    cargo semver-checks {{args}}

# Start a test database
start:  (docker-up 'db') docker-is-ready

# Start a legacy test database
start-legacy:  (docker-up 'db-legacy') docker-is-ready

# Start test server for testing HTTP pmtiles
start-pmtiles-server:
    {{dockercompose}} up -d fileserver

# Start an ssl-enabled test database
start-ssl:  (docker-up 'db-ssl') docker-is-ready

# Start an ssl-enabled test database that requires a client certificate
start-ssl-cert:  (docker-up 'db-ssl-cert') docker-is-ready

# Stop the test database
stop:
    {{dockercompose}} down --remove-orphans

# Run all tests using a test database
test: start (test-cargo '--all-targets') test-doc test-frontend test-int

# Run Rust unit tests (cargo test)
test-cargo *args:
    cargo test {{args}}

# Run Rust doc tests
test-doc *args:
    cargo test --doc {{args}}

# Test code formatting
test-fmt: (cargo-install 'cargo-sort') && (fmt-toml '--check' '--check-format')
    cargo fmt --all -- --check

# Run frontend tests
[working-directory: 'martin/martin-ui']
test-frontend:
    npm run test

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

# Run all tests using the oldest supported version of the database
test-legacy: start-legacy (test-cargo '--all-targets') test-doc test-int

# Run all tests using an SSL connection to a test database. Expected output won't match.
test-ssl: start-ssl (test-cargo '--all-targets') test-doc clean-test
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

# Run typescript typechecking on the frontend
[working-directory: 'martin/martin-ui']
type-check:
    npm run type-check

# Update all dependencies, including breaking changes. Requires nightly toolchain (install with `rustup install nightly`)
update:
    cargo +nightly -Z unstable-options update --breaking
    cargo update

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

    if ! command -v file >/dev/null 2>&1; then
        missing_tools+=("file")
    fi

    if ! command -v curl >/dev/null 2>&1; then
        missing_tools+=("curl")
    fi

    if ! command -v grep >/dev/null 2>&1; then
        missing_tools+=("grep")
    fi

    if ! command -v sqlite3 >/dev/null 2>&1; then
        missing_tools+=("sqlite3")
    fi

    if ! command -v sqldiff >/dev/null 2>&1; then
        missing_tools+=("sqldiff")
    fi

    # Check Linux-specific tools
    if [[ "$OSTYPE" == "linux"* ]]; then
        if ! command -v ogrmerge.py >/dev/null 2>&1; then
            missing_tools+=("ogrmerge.py")
        fi
    fi

    # Report results
    if [[ ${#missing_tools[@]} -eq 0 ]]; then
        echo "✓ All required tools are installed"
    else
        echo "✗ Missing tools: ${missing_tools[*]}"
        echo "  Ubuntu/Debian: sudo apt install -y jq file curl grep sqlite3-tools gdal-bin"
        echo "  macOS: brew install jq file curl grep sqlite gdal"
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
    if ! command -v $COMMAND > /dev/null; then
        if ! command -v cargo-binstall > /dev/null; then
            echo "$COMMAND could not be found. Installing it with    cargo install ${INSTALL_CMD:-$COMMAND} --locked {{args}}"
            cargo install ${INSTALL_CMD:-$COMMAND} --locked {{args}}
        else
            echo "$COMMAND could not be found. Installing it with    cargo binstall ${INSTALL_CMD:-$COMMAND} {{binstall_args}} --locked {{args}}"
            cargo binstall ${INSTALL_CMD:-$COMMAND} {{binstall_args}} --locked {{args}}
        fi
    fi

# Delete test output files
[private]
clean-test:
    rm -rf tests/output

# Wait for the test database to be ready
[private]
docker-is-ready:
    {{dockercompose}} run -T --rm db-is-ready

# Start a specific test database, e.g. db or db-legacy
[private]
docker-up name: start-pmtiles-server
    {{dockercompose}} up -d {{name}}

# Install SQLX cli if not already installed.
[private]
install-sqlx:  (cargo-install 'cargo-sqlx' 'sqlx-cli' '--no-default-features' '--features' 'sqlite,native-tls')
