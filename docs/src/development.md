# Development

* Clone Martin
* Install [docker](https://docs.docker.com/get-docker/), [docker-compose](https://docs.docker.com/compose/), and [Just](https://github.com/casey/just#readme) (improved makefile processor)
* Run `just` to see available commands:

```shell, ignore
❯ git clone git@github.com:maplibre/martin.git
❯ cd martin
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
    coverage FORMAT='html' # Run code coverage on tests and save its output in the coverage directory. Parameter could be html or lcov.
    docker-build           # Build martin docker image
    docker-run *ARGS       # Build and run martin docker image
    git *ARGS              # Do any git command, ensuring that the testing environment is set up. Accepts the same arguments as git.
    print-conn-str         # Print the connection string for the test database
    lint                   # Run cargo fmt and cargo clippy
```
