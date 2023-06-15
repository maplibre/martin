# Martin

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin)
[![docs.rs docs](https://docs.rs/martin/badge.svg)](https://docs.rs/martin)
[![Slack chat](https://img.shields.io/badge/Chat-on%20Slack-blueviolet)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin.svg)](https://crates.io/crates/martin)
[![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)](https://github.com/maplibre/martin/security)
[![CI build](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)

Martin is a tile server able to generate and serve [vector tiles](https://github.com/mapbox/vector-tile-spec) on the fly from large [PostGIS](https://github.com/postgis/postgis) databases, [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new), and [MBTile](https://github.com/mapbox/mbtiles-spec) files, allowing multiple tile sources to be dynamically combined into one. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

See [Martin book](https://maplibre.org/martin/) for complete documentation.

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

## Installation
_See [installation instructions](https://maplibre.org/martin/installation.html) in the Martin book._

**Prerequisites:** If using Martin with PostgreSQL database, you must install PostGIS with at least v3.0+, v3.1+ recommended.


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

## Running Martin Service
_See [running instructions](https://maplibre.org/martin/run.html) in the Martin book._

Martin supports any number of PostgreSQL/PostGIS database connections with [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) tables and tile-producing SQL functions, as well as [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files as tile sources.

Martin can auto-discover tables and functions using a [connection string](https://maplibre.org/martin/PostgreSQL-Connection-String.html). A PG connection string can also be passed via the `DATABASE_URL` environment variable.

Each tile source will have a [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint.

#### Examples

```shell
# publish all tables and functions from a single database
export DATABASE_URL="postgresql://user:password@host:port/database"
martin

# same as above, but passing connection string via CLI, together with a directory of .mbtiles/.pmtiles files  
martin postgresql://user:password@host:port/database path/to/dir

# publish all discovered tables/funcs from two DBs
# and generate config file with all detected sources
martin postgres://... postgres://...  --save-config config.yaml

# use configuration file instead of auto-discovery
martin --config config.yaml
```

#### Docker Example
_See [Docker instructions](https://maplibre.org/martin/run-with-docker.html) in the Martin book._

Martin is also available as a [Docker image](https://ghcr.io/maplibre/martin). You could either share a configuration file from the host with the container via the `-v` param, or you can let Martin auto-discover all sources e.g. by passing `DATABASE_URL` or specifying the .mbtiles/.pmtiles files.

```shell
export PGPASSWORD=postgres  # secret!
docker run -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://user@host:port/db \
           -v /path/to/config/dir:/config \
           ghcr.io/maplibre/martin --config /config/config.yaml
```

## API
_See [API documentation](https://maplibre.org/martin/using.html) in the Martin book._

Martin data is available via the HTTP `GET` endpoints:

| URL                                    | Description                                   |
|----------------------------------------|-----------------------------------------------|
| `/`                                    | Status text, that will eventually show web UI |
| `/catalog`                             | List of all sources                           |
| `/{sourceID}`                          | Source TileJSON                               |
| `/{sourceID}/{z}/{x}/{y}`              | Map Tiles                                     |
| `/{source1},...,{sourceN}`             | Composite Source TileJSON                     |
| `/{source1},...,{sourceN}/{z}/{x}/{y}` | Composite Source Tiles                        |
| `/sprite/{spriteID}[@2x].{json,png}`   | Sprites (low and high DPI, index/png)         |
| `/health`                              | Martin server health check: returns 200 `OK`  |

## Documentation
See [Martin book](https://maplibre.org/martin/) for complete documentation.
