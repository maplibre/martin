#!/usr/bin/env just --justfile

set shell := ['bash', '-c']

# How to call the current just executable.
# Note that just_executable() may have `\` in Windows paths, so we need to quote it.
just := quote(just_executable())
dockercompose := `if docker-compose --version &> /dev/null; then echo "docker-compose"; else echo "docker compose"; fi`

# if running in CI, treat warnings as errors by setting RUSTFLAGS and RUSTDOCFLAGS to '-D warnings' unless they are already set
# Use `CI=true just ci-test` to run the same tests as in GitHub CI.
# Use `just env-info` to see the current values of RUSTFLAGS and RUSTDOCFLAGS
ci_mode := if env('CI', '') != '' {'1'} else {''}
# cargo-binstall needs a workaround due to caching
# ci_mode might be manually set by user, so re-check the env var
binstall_args := if env('CI', '') != '' {'--no-confirm --no-track --disable-telemetry'} else {''}
export RUSTFLAGS := env('RUSTFLAGS', if ci_mode == '1' {'-D warnings'} else {''})
export RUSTDOCFLAGS := env('RUSTDOCFLAGS', if ci_mode == '1' {'-D warnings'} else {''})
export RUST_BACKTRACE := env('RUST_BACKTRACE', if ci_mode == '1' {'1'} else {'0'})
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
bench:
    cargo bench --bench sources
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
bless:
    #!/usr/bin/env bash
    set -euo pipefail

    echo "Blessing unit tests"
    for target in restart clean-test bless-insta bless-frontend; do
      echo "::group::just $target"
      {{quote(just_executable())}} $target
      echo "::endgroup::"
    done

    echo "Blessing integration tests"
    {{quote(just_executable())}} bless-int

# Bless the frontend tests
[working-directory: 'martin/martin-ui']
bless-frontend:
    npm clean-install
    npm run test:update-snapshots

# Run integration tests and save its output as the new expected output
bless-insta *args:  (cargo-install 'cargo-insta')
    cargo insta test --accept --all-targets --workspace {{args}}

# Bless integration tests
bless-int:
    rm -rf tests/temp
    tests/test.sh
    rm -rf tests/expected && mv tests/output tests/expected

# Build and open mdbook documentation
book:  (cargo-install 'mdbook') (cargo-install 'mdbook-tabs')
    mdbook serve docs --open --port 8321

# Build release binaries for a target with debug info stripped
build-release target:
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_TARGET_{{shoutysnakecase(target)}}_RUSTFLAGS='-C strip=debuginfo'
    cargo build --release --target {{target}} --package mbtiles --locked
    cargo build --release --target {{target}} --package martin --locked

# Build for musl target using zigbuild
build-release-musl target:
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_TARGET_{{shoutysnakecase(target)}}_RUSTFLAGS='-C strip=debuginfo'
    cargo zigbuild --release --target {{target}} --package mbtiles --locked
    cargo zigbuild --release --target {{target}} --package martin --locked   
   

# Move release build artifacts to target_releases directory
move-artifacts target:
    mkdir -p target_releases
    mv target/{{target}}/release/martin target_releases/
    mv target/{{target}}/release/martin-cp target_releases/
    mv target/{{target}}/release/mbtiles target_releases/


# Quick compile without building a binary
check: (cargo-install 'cargo-hack')
    cargo hack --exclude-features _tiles check --all-targets --each-feature --workspace

# Test documentation generation
check-doc:  (docs '')

# Run all CI checks locally as a single command 
ci: ci-lint ci-test ci-test-publish && assert-git-is-clean

# Run all CI lint checks
ci-lint: test-fmt ci-lint-js ci-lint-rust ci-lint-deps

# Lint Rust dependencies 
ci-lint-deps: shear

# Lint and type-check the frontend 
ci-lint-js: ci-npm-install biomejs-martin-ui type-check

# Lint Rust code 
ci-lint-rust: clippy check check-doc

# Install frontend npm dependencies
[working-directory: 'martin/martin-ui']
ci-npm-install:
    npm clean-install --no-fund

# Run all CI tests
ci-test: restart ci-test-js ci-test-rust test-int env-info

# Run frontend tests 
ci-test-js: ci-npm-install test-frontend

# Test that packages can be published 
ci-test-publish:
    cargo publish --workspace --dry-run

# Run Rust unit tests by package 
ci-test-rust: start
    cargo test --package martin-tile-utils
    cargo test --package mbtiles --no-default-features
    cargo test --package mbtiles
    cargo test --package martin
    cargo test --package martin-core
    cargo test --doc

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
    docker run --rm -v ${PWD}:/workdir --entrypoint sh ghcr.io/tcort/markdown-link-check -c \
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

    echo "::group::Unit tests"
    {{just}} test-cargo --all-targets
    echo "::endgroup::"

    # echo "::group::Documentation tests"
    # {{just}} test-doc <- deliberately disabled until --doctest for cargo-llvm-cov does not hang indefinitely
    # echo "::endgroup::"

    {{just}} test-int

    cargo llvm-cov report {{args}}

# Collect build artifacts to target_releases directory
collect-artifacts target ext='':
    mkdir -p target_releases
    mv target/{{target}}/release/martin{{ext}} target_releases/
    mv target/{{target}}/release/martin-cp{{ext}} target_releases/
    mv target/{{target}}/release/mbtiles{{ext}} target_releases/

# Collect Debian package to target_releases directory
collect-deb-artifact:
    mkdir -p target_releases
    mv target/debian/debian-x86_64.deb target_releases/

# Start Martin server
cp *args:
    cargo run --bin martin-cp -- {{args}}

# Start Martin server and open a test page (not the integrated UI)
debug-page *args: start
    open tests/debug.html  # run will not exit, so open debug page first
    {{just}} run {{args}}

# Build and run martin docker image
docker-run *args:
    docker run -it --rm --net host -e DATABASE_URL -v $PWD/tests:/tests ghcr.io/maplibre/martin:1.3.0 {{args}}

# Build and open code documentation
docs *args='--open':
    DOCS_RS=1 cargo doc --no-deps {{args}} --workspace

# Print environment info
env-info:
    @echo "Running {{if ci_mode == '1' {'in CI mode'} else {'in dev mode'} }} on {{os()}} / {{arch()}}"
    @echo "PWD {{justfile_directory()}}"
    {{just}} --version
    rustc --version
    cargo --version
    rustup --version
    @echo "RUSTFLAGS='$RUSTFLAGS'"
    @echo "RUSTDOCFLAGS='$RUSTDOCFLAGS'"
    @echo "RUST_BACKTRACE='$RUST_BACKTRACE'"
    npm --version
    node --version

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
    @echo "  just book              # Build documentation"
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
      libuv1-dev \
      libz-dev

# Install macOS dependencies via Homebrew
[macos]
install-dependencies backend='vulkan':
    brew install \
        {{if backend == 'vulkan' {'molten-vk vulkan-headers'} else {''} }} \
        curl \
        glfw \
        libuv \
        zlib

# Install Windows dependencies
[windows]
install-dependencies backend='vulkan':
    @echo "rendering styles is not currently supported on windows"

# Run cargo fmt and cargo clippy
lint: fmt clippy biomejs-martin-ui type-check

# Run mbtiles command
mbtiles *args:
    cargo run -p mbtiles -- {{args}}

# Build debian package
package-deb:  (cargo-install 'cargo-deb')
    cargo deb -v -p martin --output target/debian/martin.deb

# Move Debian package to release files directory
package-deb-release:
    mkdir -p target/files
    mv target/debian-x86_64/debian-x86_64.deb target/files/martin-Debian-x86_64.deb

# Create .tar.gz package for Unix targets
package-tar target:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/files
    cd target/{{target}}
    chmod +x martin martin-cp mbtiles
    tar czvf ../files/martin-{{target}}.tar.gz martin martin-cp mbtiles

# Create .zip package for Windows
package-zip target='x86_64-pc-windows-msvc':
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p target/files
    cd target/{{target}}
    7z a ../files/martin-{{target}}.zip martin.exe martin-cp.exe mbtiles.exe

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
    {{just}} stop
    {{just}} start

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

# runs cargo-shear to lint Rust dependencies
shear:
    cargo shear --expand
    # in the future: add --deny-warnings
    # https://github.com/Boshen/cargo-shear/pull/386

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
            echo ""
            echo "::group::Resulting diff (max 100 lines)"
            diff --recursive --new-file --exclude='*.pbf' tests/output tests/expected | head -n 100 | cat --show-nonprinting
            echo "::endgroup::"
            exit 1
        else
            echo "** Expected output matches actual output"
        fi
    fi

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
    {{just}} test-cargo --all-targets
    {{just}} clean-test
    {{just}} test-doc
    tests/test.sh

# Run typescript typechecking on the frontend
[working-directory: 'martin/martin-ui']
type-check:
    npm run type-check

# Update all dependencies, including breaking changes. Requires nightly toolchain (install with `rustup install nightly`)
update:
    cargo +nightly -Z unstable-options update --breaking
    cargo update
    # Make sure that 'evil' dependencies are at the last compatible version
    # below needs to be synced with deny.toml
    cargo update --precise 1.44.3 insta
    cargo update --precise 1.24.0 libdeflater
    cargo update --precise 1.24.0 libdeflate-sys

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

    # Check Darwin-specific tools
    if [[ "$OSTYPE" == "darwin"* ]]; then
        if ! command -v gsed >/dev/null 2>&1; then
            missing_tools+=("gsed")
        fi
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
        echo "  macOS: brew install jq file curl grep sqlite gdal gsed"
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
            echo "$COMMAND could not be found. Installing it with    cargo binstall ${INSTALL_CMD:-$COMMAND} {{binstall_args}} --locked"
            cargo binstall ${INSTALL_CMD:-$COMMAND} {{binstall_args}} --locked
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
