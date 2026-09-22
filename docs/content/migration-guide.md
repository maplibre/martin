---
icon: material/update
tags:
  - configuration
  - deployment
---

# Migration Guide

Use this guide to update an existing Martin setup after a major release. See the [changelog](changelog.md) for other changes.

## From 1.x to 2.0

Martin 2.0 removes deprecated options and changes several defaults. Existing routes and legacy URL redirects remain available when their Cargo features are enabled. The configuration file format is unchanged apart from the keys and substitution syntax below.

Review [`martin cp`](#martin-cp-is-now-martin-cp), the [terminal dashboard](#the-terminal-dashboard-is-on-by-default), the [web UI](#the-web-ui-is-served-to-localhost-by-default), [cache keys](#cache-sizes-have-one-spelling) and the [MBTiles schema](#normalized-mbtiles-files-use-tiles_shallow-and-tiles_data) even if 1.x showed no deprecation warnings.

### `martin-cp` is now `martin cp`

Replace `martin-cp` with `martin cp` in scripts and commands. The copy options and configuration file still apply, subject to the changes below. Release tarballs, Docker and Homebrew no longer include a separate `martin-cp` binary. The `mbtiles` binary is unchanged.

```bash
# 1.x
martin-cp --output-file tileset.mbtiles --source my_source postgres://postgres@localhost/db
# 2.0
martin cp --output-file tileset.mbtiles --source my_source postgres://postgres@localhost/db
```

See [Generating Tiles in Bulk](martin-cp.md).

### The terminal dashboard is on by default

Remove `--tui`. Martin now shows the dashboard when stdout is a terminal. Use `--no-tui` for the log stream instead. When stdout is not a terminal, as in a service or pipe, Martin keeps the plain log automatically.

The dashboard's log pane follows `RUST_LOG_FORMAT` and defaults to `pretty`.

See [Terminal dashboard](run-with-cli.md#terminal-dashboard).

### The web UI is served to localhost by default

`--webui` now defaults to `enable` instead of `disable`. Loopback clients get the web UI at `/`. Other clients get the same text response as with `disable`. Set `--webui disable` or `web_ui: disable` to restore the 1.x behavior. `--webui enable-for-all` is unchanged.

!!! note "Reverse proxies and containers"
    Martin checks the TCP peer address, ignoring forwarded headers. A proxy connecting over loopback exposes the web UI to its clients unless you set `--webui disable`. With Docker bridge networking, connections from the host are not loopback. Use `--webui enable-for-all` to expose the UI as described in [Running with Docker](run-with-docker.md).

### PostgreSQL settings come from the command line or the configuration file

Replace these environment variables with command-line options or configuration keys.

| 1.x environment variable   | 2.0 command line                                 | 2.0 configuration file         |
| -------------------------- | ------------------------------------------------ | ------------------------------ |
| `DATABASE_URL`             | the connection string as a positional argument   | `postgres.connection_string`   |
| `DEFAULT_SRID`             | `--default-srid`                                 | `postgres.default_srid`        |
| `PGSSLCERT`                | `--ssl-cert`                                     | `postgres.ssl_cert`            |
| `PGSSLKEY`                 | `--ssl-key`                                      | `postgres.ssl_key`             |
| `PGSSLROOTCERT`            | `--ca-root-file`                                 | `postgres.ssl_root_cert`       |

Martin ignores the old variables. Starting with only `DATABASE_URL` exported fails with `No tile sources found`. Without replacement settings, auto-discovered tables with SRID 0 are skipped with a warning, and client certificates previously supplied through `PGSSLCERT` are not sent.

```bash
# 1.x
export DATABASE_URL=postgres://postgres@localhost/db
martin
# 2.0
martin postgres://postgres@localhost/db
```

To keep using an environment variable, reference it in the configuration file. See [Configuration](config-file/index.md) for the substitution syntax.

```yaml
postgres:
  connection_string: ${DATABASE_URL}
```

With Docker, pass the connection string after the image name or pass the variable to a mounted configuration file.

```bash
# 1.x
docker run -p 3000:3000 -e DATABASE_URL=postgres://postgres@postgres.example.org/db ghcr.io/maplibre/martin
# 2.0
docker run -p 3000:3000 ghcr.io/maplibre/martin postgres://postgres@postgres.example.org/db
# 2.0, keeping the variable
docker run -p 3000:3000 -e DATABASE_URL -v "$PWD/config.yaml:/config.yaml" ghcr.io/maplibre/martin --config /config.yaml
```

See [Environment Variables](env-vars.md) and [SSL Connections](pg-connections/index.md#ssl-connections).

### Default values in the configuration file use `${VAR:-default}`

Replace `${VAR:default}` with `${VAR:-default}`. The old syntax fails at startup with a substitution error.

```yaml
# 1.x
connection_string: ${DATABASE_URL:postgres://postgres@localhost/db}
# 2.0
connection_string: ${DATABASE_URL:-postgres://postgres@localhost/db}
```

### Cache sizes have one spelling

Replace the old cache keys below. Martin reports them as unrecognized and ignores their values, so a cache can fall back to its default size. Most migrated silently in 1.x unless both spellings were set. `pmtiles.dir_cache_size_mb` was already ignored with a deprecation warning.

| 1.x                                 | 2.0                                 |
| ----------------------------------- | ----------------------------------- |
| `cache_size_mb`                     | `cache.size_mb`                     |
| `tile_cache_size_mb`                | `cache.tile_size_mb`                |
| `fonts.cache_size_mb`               | `fonts.cache.size_mb`               |
| `sprites.cache_size_mb`             | `sprites.cache.size_mb`             |
| `pmtiles.directory_cache_size_mb`   | `pmtiles.directory_cache.size_mb`   |
| `pmtiles.dir_cache_size_mb`         | `pmtiles.directory_cache.size_mb`   |

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

Replace these legacy settings with configuration keys directly under `pmtiles` or `cog`. Old environment variables are ignored silently. Old configuration keys are reported as unrecognized.

| 1.x                                                                                                          | 2.0                                                               |
| ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------- |
| `AWS_S3_FORCE_PATH_STYLE=1`, `aws_s3_force_path_style: true`, `force_path_style: true`                       | `virtual_hosted_style_request: false` (the meaning is inverted)   |
| `AWS_SKIP_CREDENTIALS=1`, `AWS_NO_CREDENTIALS=1`, `aws_skip_credentials: true`, `aws_no_credentials: true`   | `skip_signature: true`                                            |
| `AWS_PROFILE=name`                                                                                           | `profile: name`                                                   |

The standard `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` and `AWS_REGION` variables still work.

```yaml
pmtiles:
  skip_signature: true
  sources:
    tiles: s3://public-bucket/tiles.pmtiles
```

A public bucket previously accessed with `AWS_SKIP_CREDENTIALS=1` can now fail during instance credential lookup outside AWS. The error does not name the removed variable. Set `skip_signature: true` for unsigned requests. This requires a configuration file, including when replacing a command such as `martin s3://public-bucket/tiles.pmtiles`.

See [Remote files and prefixes](sources-pmtiles.md#remote-files-and-prefixes).

### Plain `http://` sources must opt in

Set `allow_http: true` in the configuration file for PMTiles or COG sources using `http://`. It now defaults to `false`, so plain HTTP sources are refused at startup with an error naming the option. HTTPS URLs are unaffected.

```yaml
pmtiles:
  allow_http: true
  sources:
    tiles: http://tiles.internal/tiles.pmtiles
```

The bare command `martin http://tiles.internal/tiles.pmtiles` needs a configuration file to enable `allow_http`.

See [HTTP(S)](sources-pmtiles.md#https).

### Normalized MBTiles files use `tiles_shallow` and `tiles_data`

New files created with `mbtiles copy --dst-type normalized` or `martin cp --mbtiles-type normalized` use `tiles_shallow` and `tiles_data`, the layout [Planetiler](https://github.com/onthegomap/planetiler) writes. This is also the `martin cp` default. Both layouts expose the standard `tiles` view, so readers using that view keep working.

Existing `map` and `images` files remain readable. Copying one to a new file without `--dst-type` preserves its schema. The CLI no longer offers that layout when converting from another schema.

New normalized files have no per-tile hashes or `tiles_with_hash` view. `mbtiles validate` checks tile references instead. `agg_tiles_hash` still covers the content. Choose `flat-with-hash` if you need per-tile hashes.

See [MBTiles Schemas](mbtiles-schema.md#normalized).

### Rendering is not part of the default build

For server-side style rendering on Linux, use the `-full` Docker image (`:latest-full` or `:<version>-full`), a `-full` Linux-gnu tarball, or `cargo install martin --features rendering`. The default builds no longer include rendering.

Rendering is stable as of 2.0. The configuration is unchanged. Set `styles.rendering: true` to enable it, or keep your existing rendering options.

See [Server-side raster tile rendering](sources-styles/rendering.md) and [Installation](installation.md).

### For users of the crates

- In `martin-tile-utils`, `TileCoord` fields are private. Construct coordinates with `TileCoord::new_checked` or `TileCoord::new_unchecked` and read them with `z()`, `x()` and `y()`.
- `TileData` is now `bytes::Bytes` instead of `Vec<u8>`, including data returned by `mbtiles::Mbtiles::stream_tiles`.
- In `martin-core`, `Source` is no longer dyn compatible. `clone_source` and `cancel_registry` were removed. Sources use the closed `AnySource` enum, and `BoxedSource` is now `Arc<AnySource>`.
- `CatalogSourceEntry` gained `tile_grid`, `PostgresSource::new` requires the source's `TileGrid`, and `CogError::TooManyImages` was removed.
- `mbtiles::Mbtiles::insert_tiles` now requires tile data to implement `Sync`. The copier writes the normalized layout described above.

See the [martin-core](https://github.com/maplibre/martin/blob/main/martin-core/CHANGELOG.md) and [mbtiles](https://github.com/maplibre/martin/blob/main/mbtiles/CHANGELOG.md) changelogs for crate releases.
