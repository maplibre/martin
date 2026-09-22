---
icon: material/update
tags:
  - configuration
  - deployment
---

# Migration Guide

This page lists the changes in a Martin release that can stop an existing setup from starting or alter what it does, and what to change in return.
Everything else that changed is in the [changelog](changelog.md).
Each section covers one major version.

## From 1.x to 2.0

Martin 2.0 removes what 1.x had deprecated and changes a few defaults.
The HTTP API is unchanged, so every 1.x route, including the redirects for old URLs, answers as before.
The configuration file format is unchanged apart from the keys listed below.

If your 1.x log showed no deprecation warning, the sections on [`martin cp`](#martin-cp-is-now-martin-cp), the [terminal dashboard](#the-terminal-dashboard-is-on-by-default), the [web UI](#the-web-ui-is-served-to-localhost-by-default), [cache keys](#cache-sizes-have-one-spelling) and the [MBTiles schema](#normalized-mbtiles-files-use-tiles_shallow-and-tiles_data) are still worth a look, because 1.x did not warn about them.

### `martin-cp` is now `martin cp`

The `martin-cp` binary is gone.
Bulk tile generation is the `cp` subcommand of `martin`, with the same options and the same configuration file.
Release tarballs, the Docker image and the Homebrew formula no longer contain a `martin-cp` binary, and the `mbtiles` binary is unchanged.

```bash
# 1.x
martin-cp --output-file tileset.mbtiles --source my_source postgres://postgres@localhost/db
# 2.0
martin cp --output-file tileset.mbtiles --source my_source postgres://postgres@localhost/db
```

See [Generating Tiles in Bulk](martin-cp.md).

### The terminal dashboard is on by default

Started from an interactive terminal, Martin shows the dashboard that `--tui` used to opt into.
`martin --tui` is no longer accepted, so drop the flag.
`martin --no-tui` prints the log stream instead.
A stdout that is not a terminal, such as a service, a container or a pipe, gets the plain log without any flag, so a deployed Martin keeps its logs.
The dashboard's log pane follows `RUST_LOG_FORMAT`, `pretty` by default.

See [Terminal dashboard](run-with-cli.md#terminal-dashboard).

### The web UI is served to localhost by default

`--webui` defaulted to `disable` in 1.x.
It now defaults to `enable`, which serves the web UI at `/` to connections from the loopback interface and answers every other client with the same text `disable` gives everyone.
`--webui disable`, or `web_ui: disable` in the configuration file, restores the 1.x behavior, and `--webui enable-for-all` is unchanged.

!!! note "Reverse proxies and containers"
    The check looks at the address of the TCP peer, not at forwarded headers.
    A reverse proxy on the same host connects from loopback, so its clients see the web UI unless you pass `--webui disable`.
    Inside a container, connections from the host are not loopback, so the web UI stays hidden until you pass `--webui enable-for-all`, as described in [Running with Docker](run-with-docker.md).

### PostgreSQL settings come from the command line or the configuration file

Martin no longer reads `DATABASE_URL`, `DEFAULT_SRID`, `PGSSLCERT`, `PGSSLKEY` or `PGSSLROOTCERT` from its environment.
A `martin` started with only `DATABASE_URL` exported stops with `No tile sources found`.
The other four go quiet instead.
A table with SRID 0 is skipped with its usual warning, and a client certificate that used to come in through `PGSSLCERT` is not sent.

| 1.x environment variable | 2.0 command line                               | 2.0 configuration file       |
|--------------------------|------------------------------------------------|------------------------------|
| `DATABASE_URL`           | the connection string as a positional argument | `postgres.connection_string` |
| `DEFAULT_SRID`           | `--default-srid`                               | `postgres.default_srid`      |
| `PGSSLCERT`              | `--ssl-cert`                                   | `postgres.ssl_cert`          |
| `PGSSLKEY`               | `--ssl-key`                                    | `postgres.ssl_key`           |
| `PGSSLROOTCERT`          | `--ca-root-file`                               | `postgres.ssl_root_cert`     |

```bash
# 1.x
export DATABASE_URL=postgres://postgres@localhost/db
martin
# 2.0
martin postgres://postgres@localhost/db
```

To keep reading the variable, name it in a configuration file.
A configuration file can read any environment variable, see [Configuration](config-file/index.md) for the syntax.

```yaml
postgres:
  connection_string: ${DATABASE_URL}
```

With Docker, the connection string goes after the image name, or the variable is passed through to a mounted configuration file.

```bash
# 1.x
docker run -p 3000:3000 -e DATABASE_URL=postgres://postgres@postgres.example.org/db ghcr.io/maplibre/martin
# 2.0
docker run -p 3000:3000 ghcr.io/maplibre/martin postgres://postgres@postgres.example.org/db
# 2.0, keeping the variable
docker run -p 3000:3000 -e DATABASE_URL -v $PWD/config.yaml:/config.yaml ghcr.io/maplibre/martin --config /config.yaml
```

See [Environment Variables](env-vars.md) and [SSL Connections](pg-connections/index.md#ssl-connections).

### Default values in the configuration file use `${VAR:-default}`

The single-colon form `${VAR:default}` no longer parses.
A configuration file that still uses it fails at startup with a substitution error.
Use `${VAR:-default}`.

```yaml
# 1.x
connection_string: ${DATABASE_URL:postgres://postgres@localhost/db}
# 2.0
connection_string: ${DATABASE_URL:-postgres://postgres@localhost/db}
```

### Cache sizes have one spelling

The cache size keys 1.x migrated for you are no longer recognized.
An old key is reported as an unrecognized key, like any typo, and that cache runs at its default size.
1.x only warned when both spellings were set, so a configuration file can hit this without ever having warned.

| 1.x                               | 2.0                               |
|-----------------------------------|-----------------------------------|
| `cache_size_mb`                   | `cache.size_mb`                   |
| `tile_cache_size_mb`              | `cache.tile_size_mb`              |
| `fonts.cache_size_mb`             | `fonts.cache.size_mb`             |
| `sprites.cache_size_mb`           | `sprites.cache.size_mb`           |
| `pmtiles.directory_cache_size_mb` | `pmtiles.directory_cache.size_mb` |
| `pmtiles.dir_cache_size_mb`       | `pmtiles.directory_cache.size_mb` |

```yaml
# 1.x
cache_size_mb: 1024
pmtiles:
  directory_cache_size_mb: 128
# 2.0
cache:
  size_mb: 1024
pmtiles:
  directory_cache:
    size_mb: 128
```

See the `cache` section of the [full configuration](config-file/index.md#full-configuration).

### Object storage options use their `object_store` names

The environment variables `AWS_S3_FORCE_PATH_STYLE`, `AWS_SKIP_CREDENTIALS`, `AWS_NO_CREDENTIALS` and `AWS_PROFILE` are no longer read, and the configuration keys `aws_s3_force_path_style`, `force_path_style`, `aws_skip_credentials` and `aws_no_credentials` are no longer migrated.
A leftover variable is ignored silently, and a leftover key is reported as unrecognized.
The standard `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` and `AWS_REGION` are still read.

| 1.x                                                                                                        | 2.0                                                             |
|------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------|
| `AWS_S3_FORCE_PATH_STYLE=1`, `force_path_style: true`                                                      | `virtual_hosted_style_request: false` (the meaning is inverted) |
| `AWS_SKIP_CREDENTIALS=1`, `AWS_NO_CREDENTIALS=1`, `aws_skip_credentials: true`, `aws_no_credentials: true` | `skip_signature: true`                                          |
| `AWS_PROFILE=name`                                                                                         | `profile: name`                                                 |

The new keys go directly under `pmtiles` or `cog`.

```yaml
pmtiles:
  skip_signature: true
  sources:
    tiles: s3://public-bucket/tiles.pmtiles
```

Off AWS, a public bucket that leaned on `AWS_SKIP_CREDENTIALS=1` now fails at startup while looking up instance credentials, and the error does not name the variable.
`skip_signature: true` is the fix.
Because `skip_signature` is a configuration file option, `martin s3://public-bucket/tiles.pmtiles` cannot ask for unsigned requests on its own and needs a configuration file.

See [Remote files and prefixes](sources-pmtiles.md#remote-files-and-prefixes).

### Plain `http://` sources must opt in

`allow_http` defaults to `false` for PMTiles and COG sources.
An `http://` URL without it is refused at startup with an error that names the option.
`https://` URLs are not affected.

```yaml
pmtiles:
  allow_http: true
  sources:
    tiles: http://tiles.internal/tiles.pmtiles
```

`allow_http` can only be set in the configuration file, so `martin http://tiles.internal/tiles.pmtiles` no longer works as a bare command line.

See [HTTP(S)](sources-pmtiles.md#https).

### Normalized MBTiles files use `tiles_shallow` and `tiles_data`

`mbtiles copy --dst-type normalized` and `martin cp --mbtiles-type normalized`, which is the `martin cp` default, create the `tiles_shallow` and `tiles_data` layout that [Planetiler](https://github.com/onthegomap/planetiler) writes, instead of `map` and `images`.
The standard `tiles` view is present in both layouts, so any MBTiles reader keeps working.
Files with `map` and `images` are still read, and a copy of such a file made without `--dst-type` keeps its schema.
Nothing on the command line creates `map` and `images` from another schema anymore.

New normalized files store no per-tile hash.
`mbtiles validate` checks their foreign keys instead, no `tiles_with_hash` view is created, and `agg_tiles_hash` still covers the content.
Pick `flat-with-hash` if you need per-tile hashes.

See [MBTiles Schemas](mbtiles-schema.md#normalized).

### Rendering is not part of the default build

Server-side style rendering is no longer a default Cargo feature.
`cargo install martin`, the default release tarballs and the default Docker image do not render.
Use the `-full` Docker image variant, tagged `:latest-full` or `:<version>-full`, the matching `-full` Linux-gnu tarball, or `cargo install martin --features rendering`.
The configuration does not change.
`styles.rendering: true` turns it on, and rendering is a stable feature as of 2.0.

See [Server-side raster tile rendering](sources-styles/rendering.md) and [Installation](installation.md).

### For users of the crates

Martin 2.0.0 ships with `martin-core` 0.12, `mbtiles` 0.20 and `martin-tile-utils` 0.8.

- In `martin-tile-utils`, the fields of `TileCoord` are private.
  Construct one with `TileCoord::new_checked` or `TileCoord::new_unchecked` and read it with `z()`, `x()` and `y()`.
- In `martin-core`, the `Source` trait is no longer dyn compatible and lost `clone_source` and `cancel_registry`.
  Sources are dispatched through the closed `AnySource` enum, and `BoxedSource` is `Arc<AnySource>`.
  `CatalogSourceEntry` gained a `tile_grid` field, `PostgresSource::new` takes the source's `TileGrid`, and `CogError::TooManyImages` is gone.
- In `mbtiles`, the API is compatible, but the normalized files the copier writes have the `tiles_shallow` and `tiles_data` layout described above.

The [martin-core](https://github.com/maplibre/martin/blob/main/martin-core/CHANGELOG.md) and [mbtiles](https://github.com/maplibre/martin/blob/main/mbtiles/CHANGELOG.md) changelogs list every change.
