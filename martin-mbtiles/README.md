# Development

Any changes to SQL commands require running of `just prepare-sqlite`.  This will install `cargo sqlx` command if it is not already installed, and update the `./sqlx-data.json` file.
