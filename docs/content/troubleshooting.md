# Troubleshooting

Log levels are controlled on a per-module basis, and by default all logging is disabled except for errors.
Logging is controlled via the `RUST_LOG` environment variable.
The value of this environment variable is a comma-separated list of logging directives.

This will enable debug logging for all modules:

```bash
export RUST_LOG=debug
export RUST_LOG_FORMAT=pretty
martin postgres://postgres@localhost/db
```

While this will only enable verbose logging for the `actix_web` module and enable debug logging for the `martin`
and `tokio_postgres` modules:

```bash
export RUST_LOG=actix_web=info,martin=debug,tokio_postgres=debug
export RUST_LOG_FORMAT=pretty
martin postgres://postgres@localhost/db
```

We also allow you to control the format of the logs via the `RUST_LOG_FORMAT` environment variable.
For troubleshooting, `pretty` or `full` are likely the most useful formats.
