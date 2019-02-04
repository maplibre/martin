# Martin

[![CircleCI](https://img.shields.io/circleci/project/github/urbica/martin.svg?style=popout)](https://circleci.com/gh/urbica/martin)
[![Docker pulls](https://img.shields.io/docker/pulls/urbica/martin.svg)](https://hub.docker.com/r/urbica/martin)
[![Metadata](https://images.microbadger.com/badges/image/urbica/martin.svg)](https://microbadger.com/images/urbica/martin)

Martin is a [PostGIS](https://github.com/postgis/postgis) [vector tiles](https://github.com/mapbox/vector-tile-spec) server suitable for large databases. Martin is written in [Rust](https://github.com/rust-lang/rust) using [Actix](https://github.com/actix/actix-web) web framework.

![Martin](https://raw.githubusercontent.com/urbica/martin/master/mart.png)

- [Requirements](#requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Table Sources](#table-sources)
  - [Table Sources List](#table-sources-list)
  - [Table Source TileJSON](#table-source-tilejson)
  - [Table Source tiles](#table-source-tiles)
- [Function Sources](#function-sources)
  - [Function Sources List](#function-sources-list)
  - [Function Source TileJSON](#function-source-tilejson)
  - [Function Source Tiles](#function-source-tiles)
- [Configuration File](#configuration-file)
- [Using Martin with Mapbox GL JS](#using-martin-with-mapbox-gl-js)
- [Command-line Interface](#command-line-interface)
- [Environment Variables](#environment-variables)
- [Using with Docker](#using-with-docker)
- [Building from Source](#building-from-source)
- [Development](#development)

## Requirements

Martin requires PostGIS >= 2.4.0.

## Installation

You can download martin from [Github releases page](https://github.com/urbica/martin/releases).

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap urbica/tap
brew install martin
```

## Usage

Martin requires a database connection string. It can be passed as a command-line argument or as a `DATABASE_URL` environment variable.

```shell
martin postgres://postgres@localhost/db
```

## Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, martin will go through all spatial tables in the database and build a list of table sources. A table should have at least one geometry column with non-zero SRID. All other table columns will be represented as properties of a vector tile feature.

### Table Sources List

Table Sources list endpoint is available at `/index.json`

```shell
curl localhost:3000/index.json
```

### Table Source TileJSON

Table Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{schema_name}.{table_name}.json`.

For example, `points` table in `public` schema will be available at `/public.points.json`

```shell
curl localhost:3000/public.points.json
```

### Table Source tiles

Table Source tiles endpoint is available at `/{schema_name}.{table_name}/{z}/{x}/{y}.pbf`

For example, `points` table in `public` schema will be available at `/public.points/{z}/{x}/{y}.pbf`

```shell
curl localhost:3000/public.points/0/0/0.pbf
```

## Function Sources

Function Source is a database function which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, martin will look for the functions with a suitable signature. A function that takes `z integer`, `x integer`, `y integer`, and `query_params json` and returns `bytea`, can be used as a Function Source.

| Argument     | Type    | Description             |
| ------------ | ------- | ----------------------- |
| z            | integer | Tile zoom parameter     |
| x            | integer | Tile x parameter        |
| y            | integer | Tile y parameter        |
| query_params | json    | Query string parameters |

**Hint**: You may want to use [TileBBox](https://github.com/mapbox/postgis-vt-util#tilebbox) function to generate bounding-box geometry of the area covered by a tile.

Here is an example of a function that can be used as a Function Source.

```plsql
CREATE OR REPLACE FUNCTION public.function_source(z integer, x integer, y integer, query_params json) RETURNS BYTEA AS $$
DECLARE
  bounds GEOMETRY(POLYGON, 3857) := TileBBox(z, x, y, 3857);
  mvt BYTEA;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'public.function_source', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(geom, bounds, 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && bounds
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
```

### Function Sources List

Function Sources list endpoint is available at `/rpc/index.json`

```shell
curl localhost:3000/rpc/index.json
```

### Function Source TileJSON

Function Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/rpc/{schema_name}.{function_name}.json`

For example, `points` function in `public` schema will be available at `/rpc/public.points.json`

```shell
curl localhost:3000/rpc/public.points.json
```

### Function Source Tiles

Function Source tiles endpoint is available at `/rpc/{schema_name}.{function_name}/{z}/{x}/{y}.pbf`

For example, `points` function in `public` schema will be available at `/rpc/public.points/{z}/{x}/{y}.pbf`

```shell
curl localhost:3000/rpc/public.points/0/0/0.pbf
```

## Configuration File

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file. To start martin with a configuration file you need to pass a file name with a `--config` argument.

```shell
martin --config config.yaml
```

You can find an example of a configuration file [here](https://github.com/urbica/martin/blob/master/tests/config.yaml).

## Using Martin with Mapbox GL JS

[Mapbox GL JS](https://github.com/mapbox/mapbox-gl-js) is a JavaScript library for interactive, customizable vector maps on the web. It takes map styles that conform to the
[Mapbox Style Specification](https://www.mapbox.com/mapbox-gl-js/style-spec), applies them to vector tiles that
conform to the [Mapbox Vector Tile Specification](https://github.com/mapbox/vector-tile-spec), and renders them using
WebGL.

You can add a layer to the map and specify martin TileJSON endpoint as a vector source URL. You should also specify a `source-layer` property. For Table Sources it is `{schema_name}.{table_name}` by default.

```js
map.addLayer({
  id: 'public.points',
  type: 'circle',
  source: {
    type: 'vector',
    url: 'http://localhost:3000/public.points.json'
  },
  'source-layer': 'public.points'
});
```

## Command-line Interface

You can configure martin using command-line interface

```shell
Usage:
  martin [options] [<connection>]
  martin -h | --help
  martin -v | --version

Options:
  -h --help               Show this screen.
  -v --version            Show version.
  --workers=<n>           Number of web server workers.
  --pool_size=<n>         Maximum connections pool size [default: 20].
  --keep_alive=<n>        Connection keep alive timeout [default: 75].
  --listen_addresses=<n>  The socket address to bind [default: 0.0.0.0:3000].
  --config=<path>         Path to config file.
```

## Environment Variables

You can also configure martin using environment variables

| Environment variable | Example                          | Description                   |
| -------------------- | -------------------------------- | ----------------------------- |
| DATABASE_URL         | postgres://postgres@localhost/db | postgres database connection  |
| DATABASE_POOL_SIZE   | 20                               | maximum connections pool size |
| WORKER_PROCESSES     | 8                                | number of web server workers  |
| KEEP_ALIVE           | 75                               | connection keep alive timeout |

## Using with Docker

You can use official Docker image [`urbica/martin`](https://hub.docker.com/r/urbica/martin)

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgres://postgres@localhost/db \
  urbica/martin
```

If you are running PostgreSQL instance on `localhost`, you have to change network settings to allow the Docker container to access the `localhost` network.

For Linux, add the `--net=host` flag to access the `localhost` PostgreSQL service.

```shell
docker run \
  --net=host \
  -p 3000:3000 \
  -e DATABASE_URL=postgres://postgres@localhost/db \
  urbica/martin
```

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgres://postgres@host.docker.internal/db \
  urbica/martin
```

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgres://postgres@docker.for.win.localhost/db \
  urbica/martin
```

## Building from Source

You can clone the repository and build martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

```shell
git clone git@github.com:urbica/martin.git
cd martin
cargo build --release
```

The binary will be available at `./target/release/martin`.

```shell
cd ./target/release/
./martin postgres://postgres@localhost/db
```

## Development

Install project dependencies and check if all the tests are running.

```shell
cargo test
cargo run
```
