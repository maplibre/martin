---
description: Logging with RUST_LOG, Prometheus metrics, fixing common problems and getting help from the community
icon: material/lifebuoy
tags:
  - troubleshooting
---

# Troubleshooting

## Logging

By default, Martin's own modules (`martin` and `martin_core`) log at `info` level.
When something goes wrong, enable debug logging for all modules and pick a readable format:

```bash
export RUST_LOG=debug
export RUST_LOG_FORMAT=pretty
martin postgres://postgres@localhost/db
```

!!! tip "Choosing what `RUST_LOG` logs"

    `RUST_LOG` is a comma-separated list of directives.
    Each sets the log level for one module, or for all modules when it names none.
    This enables `info` logging for the `actix_web` module and debug logging for the `martin` and `tokio_postgres` modules:

    ```bash
    export RUST_LOG=actix_web=info,martin=debug,tokio_postgres=debug
    ```

!!! tip "Log formats"

    `RUST_LOG_FORMAT` controls how each log line is written.
    For troubleshooting, `pretty` or `full` are likely the most useful.

    | Value                   | Output                                                                                  |
    |-------------------------|-----------------------------------------------------------------------------------------|
    | `compact`               | Single-line, optimized for short lines. The default.                                    |
    | `full`                  | Single-line, with all span fields.                                                      |
    | `pretty` (or `verbose`) | Multi-line and colored. The default in the terminal dashboard and in debug builds.      |
    | `bare`                  | Short lines without timestamps, spans, locations or colors.                             |
    | `json` (or `jsonl`)     | Newline-delimited JSON, for log collectors.                                             |

## Metrics

Martin serves [Prometheus](https://prometheus.io/) metrics at `/_/metrics` (behind `route_prefix` if one is set):

- `martin_http_requests_total` and `martin_http_requests_duration_seconds` count requests and their latency per endpoint, method and status.
  Requests to paths Martin does not serve are reported as `endpoint="UNKNOWN"`.
- `martin_tile_cache_requests_total` counts tile cache hits and misses per cache and zoom.

To tell several Martin instances apart, add labels to every metric in the [configuration file](config-file/index.md):

```yaml
observability:
  metrics:
    add_labels:
      env: prod
      # environment variables are expanded, with an optional default after `:-`
      server: ${HOSTNAME:-martin}
```

## Getting help

Stuck? People from the MapLibre community are happy to help:

- Ask in the `#maplibre-martin` channel on the [OSMUS Slack](https://slack.openstreetmap.us/) (the link sends you an automatic invite).
- Ask a question or share what you built in [GitHub Discussions](https://github.com/maplibre/martin/discussions).
- Report a bug or request a feature in [GitHub Issues](https://github.com/maplibre/martin/issues).
  An error message or log line that left you confused is a bug too, so please report it.

Including your Martin version (`martin --version`), the relevant part of your configuration and a debug log (see [Logging](#logging)) makes it much easier for others to help.
