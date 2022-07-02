# Martin

[![CI](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)
![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)
[![Docker pulls](https://img.shields.io/docker/pulls/urbica/martin.svg)](https://hub.docker.com/r/urbica/martin)

Martin is a [PostGIS](https://github.com/postgis/postgis) [vector tiles](https://github.com/mapbox/vector-tile-spec) server suitable for large databases. Martin is written in [Rust](https://github.com/rust-lang/rust) using [Actix](https://github.com/actix/actix-web) web framework.

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

- [Requirements](#requirements)
- [Installation](#installation)
- [Usage](#usage)
- [API](#api)
- [Using with Mapbox GL JS](#using-with-mapbox-gl-js)
- [Using with Leaflet](#using-with-leaflet)
- [Using with deck.gl](#using-with-deckgl)
- [Table Sources](#table-sources)
  - [Table Sources List](#table-sources-list)
  - [Table Source TileJSON](#table-source-tilejson)
  - [Table Source Tiles](#table-source-tiles)
- [Composite Sources](#composite-sources)
  - [Composite Source TileJSON](#composite-source-tilejson)
  - [Composite Source Tiles](#composite-source-tiles)
- [Function Sources](#function-sources)
  - [Function Sources List](#function-sources-list)
  - [Function Source TileJSON](#function-source-tilejson)
  - [Function Source Tiles](#function-source-tiles)
- [Command-line Interface](#command-line-interface)
- [Environment Variables](#environment-variables)
- [Configuration File](#configuration-file)
- [Using with Docker](#using-with-docker)
- [Using with Docker Compose](#using-with-docker-compose)
- [Using with Nginx](#using-with-nginx)
  - [Rewriting URLs](#rewriting-urls)
  - [Caching tiles](#caching-tiles)
- [Building from Source](#building-from-source)
- [Debugging](#debugging)
- [Development](#development)
- [Recipes](#recipes)
  - [Using with DigitalOcean PostgreSQL](#using-with-digitalocean-postgresql)
  - [Using with Heroku PostgreSQL](#using-with-heroku-postgresql)

## Requirements

Martin requires PostGIS >= 2.4.0.

## Installation

You can download martin from [Github releases page](https://github.com/maplibre/martin/releases).

| Platform | Downloads (latest)      |
| -------- | ----------------------- |
| Linux    | [64-bit][rl-linux-tar]  |
| macOS    | [64-bit][rl-macos-tar]  |
| Windows  | [64-bit][rl-winx64-zip] |

[rl-linux-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Linux-x86_64.tar.gz
[rl-macos-tar]: https://github.com/maplibre/martin/releases/latest/download/martin-Darwin-x86_64.tar.gz
[rl-winx64-zip]: https://github.com/maplibre/martin/releases/latest/download/martin-Windows-x86_64.zip

If you are using macOS and [Homebrew](https://brew.sh/) you can install martin using Homebrew tap.

```shell
brew tap urbica/tap
brew install martin
```

You can also use [official Docker image](https://hub.docker.com/r/urbica/martin)

```shell
docker run -p 3000:3000 -e DATABASE_URL=postgres://postgres@localhost/db urbica/martin
```

## Usage

Martin requires a database connection string. It can be passed as a command-line argument or as a `DATABASE_URL` environment variable.

```shell
martin postgres://postgres@localhost/db
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for each [geospatial-enabled](https://postgis.net/docs/postgis_usage.html#geometry_columns) table in your database.

## API

When started, martin will go through all spatial tables and functions with an appropriate signature in the database. These tables and functions will be available as the HTTP endpoints, which you can use to query Mapbox vector tiles.

| Method | URL                                                                              | Description                                             |
| ------ | -------------------------------------------------------------------------------- | ------------------------------------------------------- |
| `GET`  | `/index.json`                                                                    | [Table Sources List](#table-sources-list)               |
| `GET`  | `/{schema_name}.{table_name}.json`                                               | [Table Source TileJSON](#table-source-tilejson)         |
| `GET`  | `/{schema_name}.{table_name}/{z}/{x}/{y}.pbf`                                    | [Table Source Tiles](#table-source-tiles)               |
| `GET`  | `/{schema_name1}.{table_name1},...,{schema_nameN}.{table_nameN}.json`            | [Composite Source TileJSON](#composite-source-tilejson) |
| `GET`  | `/{schema_name1}.{table_name1},...,{schema_nameN}.{table_nameN}/{z}/{x}/{y}.pbf` | [Composite Source Tiles](#composite-source-tiles)       |
| `GET`  | `/rpc/index.json`                                                                | [Function Sources List](#function-sources-list)         |
| `GET`  | `/rpc/{schema_name}.{function_name}.json`                                        | [Function Source TileJSON](#function-source-tilejson)   |
| `GET`  | `/rpc/{schema_name}.{function_name}/{z}/{x}/{y}.pbf`                             | [Function Source Tiles](#function-source-tiles)         |
| `GET`  | `/healthz`                                                                       | Martin server health check: returns `200 OK`            |

## Using with Mapbox GL JS

[Mapbox GL JS](https://github.com/mapbox/mapbox-gl-js) is a JavaScript library for interactive, customizable vector maps on the web. It takes map styles that conform to the
[Mapbox Style Specification](https://www.mapbox.com/mapbox-gl-js/style-spec), applies them to vector tiles that
conform to the [Mapbox Vector Tile Specification](https://github.com/mapbox/vector-tile-spec), and renders them using
WebGL.

You can add a layer to the map and specify martin TileJSON endpoint as a vector source URL. You should also specify a `source-layer` property. For [Table Sources](#table-sources) it is `{schema_name}.{table_name}` by default.

```js
map.addLayer({
  id: 'public.points',
  type: 'circle',
  source: {
    type: 'vector',
    url: 'http://localhost:3000/public.points.json'
  },
  'source-layer': 'public.points',
  paint: {
    'circle-color': 'red'
  }
});
```

You can also combine multiple tables into one source with [Composite Sources](#composite-sources). Each [Table Source](#table-sources) in Composite Source can be accessed with its `{schema_name}.{table_name}` as a `source-layer` property.

```js
map.addSource('points', {
  type: 'vector',
  url: `http://0.0.0.0:3000/public.points1,public.points2.json`
});

map.addLayer({
  id: 'red_points',
  type: 'circle',
  source: 'points',
  'source-layer': 'public.points1',
  paint: {
    'circle-color': 'red'
  }
});

map.addLayer({
  id: 'blue_points',
  type: 'circle',
  source: 'points',
  'source-layer': 'public.points2',
  paint: {
    'circle-color': 'blue'
  }
});
```

## Using with MapLibre
[MapLibre](https://maplibre.org/projects/maplibre-gl-js/) is an Open-source JavaScript library for publishing maps on your websites. Originated as an open-source fork of [mapbox-gl-js](https://www.mapbox.com/mapbox-gljs), [MapLibre](https://maplibre.org/projects/maplibre-gl-js/) is intended to be a drop-in replacement for the Mapbox’s version with additional functionality.
Generally There is no big difference between MapLibre and [mapbox-gl-js](https://www.mapbox.com/mapbox-gljs) when cooperating with Martin.

```js
map.addSource('rpc', {
  type: 'vector',
  url: `http://localhost:3000/rpc/public.function_source.json`
});
map.addLayer({
  id: 'points',
  type: 'circle',
  source: 'rpc',
  'source-layer': 'public.function_source',
  paint: {
    'circle-color': 'blue'
  }
});
```

## Using with Leaflet

[Leaflet](https://github.com/Leaflet/Leaflet) is the leading open-source JavaScript library for mobile-friendly interactive maps.

You can add vector tiles using [Leaflet.VectorGrid](https://github.com/Leaflet/Leaflet.VectorGrid) plugin. You must initialize a [VectorGrid.Protobuf](https://leaflet.github.io/Leaflet.VectorGrid/vectorgrid-api-docs.html#vectorgrid-protobuf) with a URL template, just like in L.TileLayers. The difference is that you should define the styling for all the features.

```js
L.vectorGrid
  .protobuf('http://localhost:3000/public.points/{z}/{x}/{y}.pbf', {
    vectorTileLayerStyles: {
      'public.points': {
        color: 'red',
        fill: true
      }
    }
  })
  .addTo(map);
```

## Using with deck.gl

[deck.gl](https://deck.gl/) is a WebGL-powered framework for visual exploratory data analysis of large datasets.

You can add vector tiles using [MVTLayer](https://deck.gl/docs/api-reference/geo-layers/mvt-layer). MVTLayer `data` property defines the remote data for the MVT layer. It can be

- `String`: Either a URL template or a [TileJSON](https://github.com/mapbox/tilejson-spec) URL.
- `Array`: an array of URL templates. It allows to balance the requests across different tile endpoints. For example, if you define an array with 4 urls and 16 tiles need to be loaded, each endpoint is responsible to server 16/4 tiles.
- `JSON`: A valid [TileJSON object](https://github.com/mapbox/tilejson-spec/tree/master/2.2.0).

```js
const pointsLayer = new MVTLayer({
  data: 'http://localhost:3000/public.points.json', // 'http://localhost:3000/public.table_source/{z}/{x}/{y}.pbf'
  pointRadiusUnits: 'pixels',
  getRadius: 5,
  getFillColor: [230, 0, 0]
});

const deckgl = new DeckGL({
  container: 'map',
  mapStyle: 'https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json',
  initialViewState: {
    latitude: 0,
    longitude: 0,
    zoom: 1
  },
  layers: [pointsLayer]
});
```

## Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, martin will go through all spatial tables in the database and build a list of table sources. A table should have at least one geometry column with non-zero SRID. All other table columns will be represented as properties of a vector tile feature.

**Note**: In case if there are multiple geometry columns in the table, you can specify the geometry column name in the table source name to access particular geometry in vector tile, e.g. `schema_name.table_name.geometry_column`.

### Table Sources List

Table Sources list endpoint is available at `/index.json`

```shell
curl localhost:3000/index.json
```

**Note**: if in `watch` mode, this will rescan database for table sources.

### Table Source TileJSON

Table Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{schema_name}.{table_name}.json`.

For example, `points` table in `public` schema will be available at `/public.points.json`

```shell
curl localhost:3000/public.points.json
```

In case if you have multiple geometry columns in that table and want to access a particular geometry column in vector tile, you should also specify the geometry column in the table source name

```shell
curl localhost:3000/public.points.geom.json
```

### Table Source Tiles

Table Source tiles endpoint is available at `/{schema_name}.{table_name}/{z}/{x}/{y}.pbf`

For example, `points` table in `public` schema will be available at `/public.points/{z}/{x}/{y}.pbf`

```shell
curl localhost:3000/public.points/0/0/0.pbf
```

In case if you have multiple geometry columns in that table and want to access a particular geometry column in vector tile, you should also specify the geometry column in the table source name

```shell
curl localhost:3000/public.points.geom/0/0/0.pbf
```

## Composite Sources

Composite Sources allows combining multiple Table Sources into one. Composite Source consists of multiple Table Sources separated by comma `{schema_name1}.{table_name1},...,{schema_nameN}.{table_nameN}`

Each [Table Source](#table-sources) in Composite Source can be accessed with its `{schema_name}.{table_name}` as a `source-layer` property.

### Composite Source TileJSON

Composite Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{schema_name1}.{table_name1},...,{schema_nameN}.{table_nameN}.json`.

For example, composite source for `points` and `lines` tables in `public` schema will be available at `/public.points,public.lines.json`

```shell
curl localhost:3000/public.points,public.lines.json
```

### Composite Source Tiles

Composite Source tiles endpoint is available at `/{schema_name1}.{table_name1},...,{schema_nameN}.{table_nameN}/{z}/{x}/{y}.pbf`

For example, composite source for `points` and `lines` tables in `public` schema will be available at `/public.points,public.lines/{z}/{x}/{y}.pbf`

```shell
curl localhost:3000/public.points,public.lines/0/0/0.pbf
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

For example, if you have a table `public.table_source` in WGS84 (`4326` SRID), then you can use this function as a Function Source:

```sql
CREATE OR REPLACE FUNCTION public.function_source(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'public.function_source', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), TileBBox(z, x, y, 3857), 4096, 64, true) AS geom
    FROM public.table_source
    WHERE geom && TileBBox(z, x, y, 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
```

The `query_params` argument is a JSON representation of the tile request query params. For example, if user requested a tile with [urlencoded](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/encodeURIComponent) params:

```shell
curl \
  --data-urlencode 'arrayParam=[1, 2, 3]' \
  --data-urlencode 'numberParam=42' \
  --data-urlencode 'stringParam=value' \
  --data-urlencode 'booleanParam=true' \
  --data-urlencode 'objectParam={"answer" : 42}' \
  --get localhost:3000/rpc/public.function_source/0/0/0.pbf
```

then `query_params` will be parsed as:

```json
{
  "arrayParam": [1, 2, 3],
  "numberParam": 42,
  "stringParam": "value",
  "booleanParam": true,
  "objectParam": { "answer": 42 }
}
```

You can access this params using [json operators](https://www.postgresql.org/docs/current/functions-json.html):

```sql
...WHERE answer = (query_params->'objectParam'->>'answer')::int;
```

### Function Sources List

Function Sources list endpoint is available at `/rpc/index.json`

```shell
curl localhost:3000/rpc/index.json
```

**Note**: if in `watch` mode, this will rescan database for function sources.

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

## Command-line Interface

You can configure martin using command-line interface

```shell
Usage:
  martin [options] [<connection>]
  martin -h | --help
  martin -v | --version

Options:
  -h --help                         Show this screen.
  -v --version                      Show version.
  --config=<path>                   Path to config file.
  --keep-alive=<n>                  Connection keep alive timeout [default: 75].
  --listen-addresses=<n>            The socket address to bind [default: 0.0.0.0:3000].
  --default-srid=<n>                If a spatial table has SRID 0, then this default SRID will be used as a fallback.
  --pool-size=<n>                   Maximum connections pool size [default: 20].
  --watch                           Scan for new sources on sources list requests.
  --workers=<n>                     Number of web server workers.
  --ca-root-file=<path>             Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates.
  --danger-accept-invalid-certs     Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
```

## Environment Variables

You can also configure martin using environment variables

| Environment variable          | Example                            | Description                                   |
| ----------------------------- | ---------------------------------- | --------------------------------------------- |
| `DATABASE_URL`                | `postgres://postgres@localhost/db` | Postgres database connection                  |
| `WATCH_MODE`                  | `true`                             | Scan for new sources on sources list requests |
| `CA_ROOT_FILE`                | `./ca-certificate.crt`             | Loads trusted root certificates from a file   |
| `DEFAULT_SRID`                | `4326`                             | Fallback SRID                                 |
| `DANGER_ACCEPT_INVALID_CERTS` | `false`                            | Trust invalid certificates                    |

## Configuration File

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file. To start martin with a configuration file you need to pass a path to a file with a `--config` argument.

```shell
martin --config config.yaml
```

You can find an example of a configuration file [here](https://github.com/maplibre/martin/blob/main/tests/config.yaml).

```yaml
# The socket address to bind [default: 0.0.0.0:3000]
listen_addresses: '0.0.0.0:3000'

# Database connection string
connection_string: 'postgres://postgres@localhost:5432/db'

# Maximum connections pool size [default: 20]
pool_size: 20

# Connection keep alive timeout [default: 75]
keep_alive: 75

# Number of web server workers
worker_processes: 8

# If a spatial table has SRID 0, then this default SRID will be used as a fallback
default_srid: 4326

# Enable watch mode
watch: false

# Trust invalid certificates. This introduces significant vulnerabilities, and should only be used as a last resort.
danger_accept_invalid_certs: false

# Associative arrays of table sources
table_sources:
  public.table_source:
    # Table source id (required)
    id: public.table_source

    # Table schema (required)
    schema: public

    # Table name (required)
    table: table_source

    # Geometry SRID (required)
    srid: 4326

    # Geometry column name (required)
    geometry_column: geom

    # Feature id column name
    id_column: ~

    # An integer specifying the minimum zoom level
    minzoom: 0

    # An integer specifying the maximum zoom level. MUST be >= minzoom
    maxzoom: 30

    # The maximum extent of available map tiles. Bounds MUST define an area
    # covered by all zoom levels. The bounds are represented in WGS:84
    # latitude and longitude values, in the order left, bottom, right, top.
    # Values may be integers or floating point numbers.
    bounds: [-180.0, -90.0, 180.0, 90.0]

    # Tile extent in tile coordinate space
    extent: 4096

    # Buffer distance in tile coordinate space to optionally clip geometries
    buffer: 64

    # Boolean to control if geometries should be clipped or encoded as is
    clip_geom: true

    # Geometry type
    geometry_type: GEOMETRY

    # List of columns, that should be encoded as tile properties (required)
    properties:
      gid: int4

# Associative arrays of function sources
function_sources:
  public.function_source:
    # Function source id (required)
    id: public.function_source

    # Schema name (required)
    schema: public

    # Function name (required)
    function: function_source

    # An integer specifying the minimum zoom level
    minzoom: 0

    # An integer specifying the maximum zoom level. MUST be >= minzoom
    maxzoom: 30

    # The maximum extent of available map tiles. Bounds MUST define an area
    # covered by all zoom levels. The bounds are represented in WGS:84
    # latitude and longitude values, in the order left, bottom, right, top.
    # Values may be integers or floating point numbers.
    bounds: [-180.0, -90.0, 180.0, 90.0]
```

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

## Using with Docker Compose

You can use example [`docker-compose.yml`](https://raw.githubusercontent.com/maplibre/martin/main/docker-compose.yml) file as a reference

```yml
version: '3'

services:
  martin:
    image: urbica/martin
    restart: unless-stopped
    ports:
      - 3000:3000
    environment:
      - DATABASE_URL=postgres://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:14-3.1-alpine
    restart: unless-stopped
    environment:
      - POSTGRES_DB=db
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      - ./pg_data:/var/lib/postgresql/data
```

First, you need to start `db` service

```shell
docker-compose up -d db
```

Then, after `db` service is ready to accept connections, you can start `martin`

```shell
docker-compose up -d martin
```

By default, martin will be available at [localhost:3000](http://localhost:3000/index.json)

## Using with Nginx

You can run martin behind Nginx proxy, so you can cache frequently accessed tiles and reduce unnecessary pressure on the database.

```yml
version: '3'

services:
  nginx:
    image: nginx:alpine
    restart: unless-stopped
    ports:
      - 80:80
    volumes:
      - ./cache:/var/cache/nginx
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on:
      - martin

  martin:
    image: urbica/martin
    restart: unless-stopped
    environment:
      - DATABASE_URL=postgres://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:14-3.1-alpine
    restart: unless-stopped
    environment:
      - POSTGRES_DB=db
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      - ./pg_data:/var/lib/postgresql/data
```

You can find an example Nginx configuration file [here](https://github.com/maplibre/martin/blob/main/nginx.conf).

### Rewriting URLs

If you are running martin behind Nginx proxy, you may want to rewrite the request URL to properly handle tile URLs in [TileJSON](#table-source-tilejson) [endpoints](#function-source-tilejson).

```nginx
location ~ /tiles/(?<fwd_path>.*) {
    proxy_set_header  X-Rewrite-URL $uri;
    proxy_set_header  X-Forwarded-Host $host:$server_port;
    proxy_set_header  X-Forwarded-Proto $scheme;
    proxy_redirect    off;

    proxy_pass        http://martin:3000/$fwd_path$is_args$args;
}
```

### Caching tiles

You can also use Nginx to cache tiles. In the example, the maximum cache size is set to 10GB, and caching time is set to 1 hour for responses with codes 200, 204, and 302 and 1 minute for responses with code 404.

```nginx
http {
  ...
  proxy_cache_path  /var/cache/nginx/
                    levels=1:2
                    max_size=10g
                    use_temp_path=off
                    keys_zone=tiles_cache:10m;

  server {
    ...
    location ~ /tiles/(?<fwd_path>.*) {
        proxy_set_header        X-Rewrite-URL $uri;
        proxy_set_header        X-Forwarded-Host $host:$server_port;
        proxy_set_header        X-Forwarded-Proto $scheme;
        proxy_redirect          off;

        proxy_cache             tiles_cache;
        proxy_cache_lock        on;
        proxy_cache_revalidate  on;

        # Set caching time for responses
        proxy_cache_valid       200 204 302 1h;
        proxy_cache_valid       404 1m;

        proxy_cache_use_stale   error timeout http_500 http_502 http_503 http_504;
        add_header              X-Cache-Status $upstream_cache_status;

        proxy_pass              http://martin:3000/$fwd_path$is_args$args;
    }
  }
}
```

You can find an example Nginx configuration file [here](https://github.com/maplibre/martin/blob/main/nginx.conf).

## Building from Source

You can clone the repository and build martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

```shell
git clone git@github.com:maplibre/martin.git
cd martin
cargo build --release
```

The binary will be available at `./target/release/martin`.

```shell
cd ./target/release/
./martin postgres://postgres@localhost/db
```

## Debugging

Log levels are controlled on a per-module basis, and by default all logging is disabled except for errors. Logging is controlled via the `RUST_LOG` environment variable. The value of this environment variable is a comma-separated list of logging directives.

This will enable debug logging for all modules:

```shell
export RUST_LOG=debug
martin postgres://postgres@localhost/db
```

While this will only enable verbose logging for the `actix_web` module and enable debug logging for the `martin` and `tokio_postgres` modules:

```shell
export RUST_LOG=actix_web=info,martin=debug,tokio_postgres=debug
martin postgres://postgres@localhost/db
```

## Development

Clone project

```shell
git clone git@github.com:maplibre/martin.git
cd martin
```

Start `db` service using [docker-compose](https://docs.docker.com/compose/)

```shell
docker-compose up -d db
```

Then, after `db` service is ready to accept connections, you can start `martin` with

```shell
DATABASE_URL=postgres://postgres@localhost/db cargo run
```

Open `tests/debug.html` for debugging. By default, martin will be available at [localhost:3000](http://localhost:3000/index.json)

Make your changes, and check if all the tests are running

```shell
DATABASE_URL=postgres://postgres@localhost/db cargo test
```

You can also run benchmarks with

```shell
DATABASE_URL=postgres://postgres@localhost/db cargo bench
```

An HTML report displaying the results of the benchmark will be generated under `target/criterion/report/index.html`

## Recipes


### Using with DigitalOcean PostgreSQL

You can use martin with [Managed PostgreSQL from DigitalOcean](https://www.digitalocean.com/products/managed-databases-postgresql/) with PostGIS extension

First, you need to download the CA certificate and get your cluster connection string from the [dashboard](https://cloud.digitalocean.com/databases). After that, you can use the connection string and the CA certificate to connect to the database

```
martin --ca-root-file ./ca-certificate.crt postgres://user:password@host:port/db?sslmode=require
```

### Using with Heroku PostgreSQL

You can use martin with [Managed PostgreSQL from Heroku](https://www.heroku.com/postgres) with PostGIS extension

```
heroku pg:psql -a APP_NAME -c 'create extension postgis'
```

In order to trust the Heroku certificate, you can disable certificate validation with either `DANGER_ACCEPT_INVALID_CERTS` environment variable

```
DATABASE_URL=$(heroku config:get DATABASE_URL -a APP_NAME) DANGER_ACCEPT_INVALID_CERTS=true martin
```

or `--danger-accept-invalid-certs` command-line argument

```
martin --danger-accept-invalid-certs $(heroku config:get DATABASE_URL -a APP_NAME)
```
