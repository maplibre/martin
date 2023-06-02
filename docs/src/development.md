# Development

* Clone Martin
```shell, ignore
git clone git@github.com:maplibre/martin.git -o upstream
cd martin
```

* Fork Martin repo into your own GitHub account, and add your fork as a remote

```shell, ignore
git remote add origin <your_fork_url>
```

* Install [docker](https://docs.docker.com/get-docker/), [docker-compose](https://docs.docker.com/compose/), and openssl:

```shell, ignore
# For Ubuntu-based distros
sudo apt install -y  docker.io  docker-compose  libssl-dev
```


* Install [Just](https://github.com/casey/just#readme) (improved makefile processor). Note that some Linux and Homebrew distros have outdated versions of Just, so you should install it from source:

```shell, ignore
cargo install just
```

* When developing MBTiles SQL code, you many need to use `just prepare-sqlite` whenever SQL queries are modified.
* Run `just` to see all available commands:

```shell, ignore
❯ just
Available recipes:
    run *ARGS              # Start Martin server and a test database
    debug-page *ARGS       # Start Martin server and open a test page
    psql *ARGS             # Run PSQL utility against the test database
    clean                  # Perform  cargo clean  to delete all build files
    start                  # Start a test database
    start-ssl              # Start an ssl-enabled test database
    start-legacy           # Start a legacy test database
    stop                   # Stop the test database
    bench                  # Run benchmark tests
    test                   # Run all tests using a test database
    test-ssl               # Run all tests using an SSL connection to a test database. Expected output won't match.
    test-legacy            # Run all tests using the oldest supported version of the database
    test-unit *ARGS        # Run Rust unit and doc tests (cargo test)
    test-int               # Run integration tests
    bless                  # Run integration tests and save its output as the new expected output
    mdbook                 # Build and open mdbook documentation
    docs                   # Build and open code documentation
    coverage FORMAT='html' # Run code coverage on tests and save its output in the coverage directory. Parameter could be html or lcov.
    docker-build           # Build martin docker image
    docker-run *ARGS       # Build and run martin docker image
    git *ARGS              # Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
    print-conn-str         # Print the connection string for the test database
    lint                   # Run cargo fmt and cargo clippy
    prepare-sqlite         # Update sqlite database schema. Install SQLX cli if not already installed.
```
