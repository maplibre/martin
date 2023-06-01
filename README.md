# Martin

[![Slack chat](https://img.shields.io/badge/Chat-on%20Slack-blueviolet)](https://slack.openstreetmap.us/)
[![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)](https://github.com/maplibre/martin/security)
[![CI build](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin.svg)](https://crates.io/crates/martin)
[![docs.rs docs](https://docs.rs/martin/badge.svg)](https://docs.rs/martin)
[![CI build](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)

Martin is a tile server able to generate [vector tiles](https://github.com/mapbox/vector-tile-spec) on the fly from large [PostGIS](https://github.com/postgis/postgis) databases, or serve tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

See [Martin book](https://maplibre.org/martin/) for complete documentation.

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

## Requirements

When using Martin with PostgreSQL, you must install PostGIS with at least v3.0+, and v3.1+ is recommended.

## Installation

You can download martin from [GitHub releases page](https://github.com/maplibre/martin/releases).

| Platform | Downloads (latest)     |
|----------|------------------------|
| Linux    | [64-bit][rl-linux-tar] |
| macOS    | [64-bit][rl-macos-tar] |
| Windows  | [64-bit][rl-win64-zip] |

[rl-linux-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Linux-x86_64.tar.gz
[rl-macos-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Darwin-x86_64.tar.gz
[rl-win64-zip]: https://github.com/maplibre/martin/releases/latest/download/martin-Windows-x86_64.zip

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap maplibre/martin https://github.com/maplibre/martin.git
brew install maplibre/martin/martin
```

You can also use [official Docker image](https://ghcr.io/maplibre/martin)

```shell
export PGPASSWORD=postgres  # secret!
docker run \
       -p 3000:3000 \
       -e PGPASSWORD \
       -e DATABASE_URL=postgresql://user@host:port/db \
       ghcr.io/maplibre/martin
```

Use docker `-v` param to share configuration file or its directory with the container:

```shell
export PGPASSWORD=postgres  # secret!
docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin --config /config/config.yaml
```

## Usage

### PostGIS sources

Martin requires at least one PostgreSQL [connection string](https://maplibre.org/martin/PostgreSQL-Connection-String.html) or a [tile source file](https://maplibre.org/martin/MBTile-and-PMTile-Sources.html) as a command-line argument. A PG connection string can also be passed via the `DATABASE_URL` environment variable.

```shell
martin postgresql://user:password@host:port/database
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for each [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) table in your database.

### MBTiles and PMTiles sources
Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files.  To serve a file from CLI, simply put the path to the file or the directory with `*.mbtiles` or `*.pmtiles` files. For example:

```shell
martin  /path/to/mbtiles/file.mbtiles
martin  /path/to/directory
```

## API

When started, Martin will go through all spatial tables and functions with an appropriate signature in the database. These tables and functions will be available as the HTTP endpoints, which you can use to query Mapbox vector tiles.

| Method | URL                                    | Description                                                                                               |
|--------|----------------------------------------|-----------------------------------------------------------------------------------------------------------|
| `GET`  | `/`                                    | Status text, that will eventually show web UI                                                             |
| `GET`  | `/catalog`                             | [List of all sources](https://maplibre.org/martin/source-list.html)                                       |
| `GET`  | `/{sourceID}`                          | [Source TileJSON](https://maplibre.org/martin/table-sources.html#table-source-tilejson)                   |
| `GET`  | `/{sourceID}/{z}/{x}/{y}`              | [Source Tiles](https://maplibre.org/martin/table-sources.html#table-source-tiles)                         |
| `GET`  | `/{sourceID1},...,{nameN}`             | [Composite Source TileJSON](https://maplibre.org/martin/composite-sources.html#composite-source-tilejson) |
| `GET`  | `/{sourceID1},...,{nameN}/{z}/{x}/{y}` | [Composite Source Tiles](https://maplibre.org/martin/composite-sources.html#composite-source-tiles)       |
| `GET`  | `/health`                              | Martin server health check: returns 200 `OK`                                                              |

## Documentation
See [Martin book](https://maplibre.org/martin/) for complete documentation.
