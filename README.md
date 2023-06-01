<h1>Martin</h1>

[![CI](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)
![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)

Martin is a tile server able to generate [vector tiles](https://github.com/mapbox/vector-tile-spec) from large [PostGIS](https://github.com/postgis/postgis) databases on the fly, or serve tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

<!-- TOC -->
* [Requirements](#requirements)
* [Installation](#installation)
* [Usage](#usage)
* [API](#api)
* [Using with MapLibre](#using-with-maplibre)
* [Using with Leaflet](#using-with-leaflet)
* [Using with deck.gl](#using-with-deckgl)
* [Using with Mapbox](#using-with-mapbox)
* [Source List](#source-list)
* [Composite Sources](#composite-sources)
  * [Composite Source TileJSON](#composite-source-tilejson)
  * [Composite Source Tiles](#composite-source-tiles)
* [Table Sources](#table-sources)
  * [Table Source TileJSON](#table-source-tilejson)
  * [Table Source Tiles](#table-source-tiles)
* [Function Sources](#function-sources)
  * [Function Source TileJSON](#function-source-tilejson)
  * [Function Source Tiles](#function-source-tiles)
* [MBTile and PMTile Sources](#mbtile-and-pmtile-sources)
* [Command-line Interface](#command-line-interface)
* [Environment Variables](#environment-variables)
* [Configuration File](#configuration-file)
* [PostgreSQL Connection String](#postgresql-connection-string)
  * [PostgreSQL SSL Connections](#postgresql-ssl-connections)
* [Using with Docker](#using-with-docker)
* [Using with Docker Compose](#using-with-docker-compose)
* [Using with Nginx](#using-with-nginx)
  * [Rewriting URLs](#rewriting-urls)
  * [Caching tiles](#caching-tiles)
* [Building from Source](#building-from-source)
* [Debugging](#debugging)
* [Development](#development)
  * [Other useful commands](#other-useful-commands)
* [Recipes](#recipes)
  * [Using with DigitalOcean PostgreSQL](#using-with-digitalocean-postgresql)
  * [Using with Heroku PostgreSQL](#using-with-heroku-postgresql)
<!-- TOC -->

# Requirements

Martin requires PostGIS 3.0+.  PostGIS 3.1+ is recommended.

# Installation

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
       -e DATABASE_URL=postgresql://postgres@localhost/db \
       ghcr.io/maplibre/martin
```

Use docker `-v` param to share configuration file or its directory with the container:

```shell
export PGPASSWORD=postgres  # secret!
docker run -v /path/to/config/dir:/config \
           -p 3000:3000 \
           -e PGPASSWORD \
           -e DATABASE_URL=postgresql://postgres@localhost/db \
           ghcr.io/maplibre/martin --config /config/config.yaml
```

# Usage

Martin requires at least one PostgreSQL [connection string](#postgresql-connection-string) or a [tile source file](#mbtile-and-pmtile-sources) as a command-line argument. A PG connection string can also be passed via the `DATABASE_URL` environment variable.

```shell
martin postgresql://postgres@localhost/db
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for each [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) table in your database.

# API

When started, Martin will go through all spatial tables and functions with an appropriate signature in the database. These tables and functions will be available as the HTTP endpoints, which you can use to query Mapbox vector tiles.

| Method | URL                                    | Description                                             |
|--------|----------------------------------------|---------------------------------------------------------|
| `GET`  | `/`                                    | Status text, that will eventually show web UI           |
| `GET`  | `/catalog`                             | [List of all sources](#source-list)                     |
| `GET`  | `/{sourceID}`                          | [Source TileJSON](#table-source-tilejson)               |
| `GET`  | `/{sourceID}/{z}/{x}/{y}`              | [Source Tiles](#table-source-tiles)                     |
| `GET`  | `/{sourceID1},...,{nameN}`             | [Composite Source TileJSON](#composite-source-tilejson) |
| `GET`  | `/{sourceID1},...,{nameN}/{z}/{x}/{y}` | [Composite Source Tiles](#composite-source-tiles)       |
| `GET`  | `/health`                              | Martin server health check: returns 200 `OK`            |

# Using with MapLibre
[MapLibre](https://maplibre.org/projects/maplibre-gl-js/) is an Open-source JavaScript library for showing maps on a website. MapLibre can accept [MVT vector tiles](https://github.com/mapbox/vector-tile-spec) generated by Martin, and applies [a style](https://maplibre.org/maplibre-gl-js-docs/style-spec/) to them to draw a map using Web GL.

You can add a layer to the map and specify Martin [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint as a vector source URL. You should also specify a `source-layer` property. For [Table Sources](#table-sources) it is `{table_name}` by default.


```js
map.addLayer({
  id: 'points',
  type: 'circle',
  source: {
    type: 'vector',
    url: 'http://localhost:3000/points'
  },
  'source-layer': 'points',
  paint: {
    'circle-color': 'red'
  },
});
```

```js
map.addSource('rpc', {
  type: 'vector',
  url: `http://localhost:3000/function_zxy_query`
});
map.addLayer({
  id: 'points',
  type: 'circle',
  source: 'rpc',
  'source-layer': 'function_zxy_query',
  paint: {
    'circle-color': 'blue'
  },
});
```

You can also combine multiple sources into one source with [Composite Sources](#composite-sources). Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

```js
map.addSource('points', {
  type: 'vector',
  url: `http://0.0.0.0:3000/points1,points2`
});

map.addLayer({
  id: 'red_points',
  type: 'circle',
  source: 'points',
  'source-layer': 'points1',
  paint: {
    'circle-color': 'red'
  }
});

map.addLayer({
  id: 'blue_points',
  type: 'circle',
  source: 'points',
  'source-layer': 'points2',
  paint: {
    'circle-color': 'blue'
  }
});
```

# Using with Leaflet

[Leaflet](https://github.com/Leaflet/Leaflet) is the leading open-source JavaScript library for mobile-friendly interactive maps.

You can add vector tiles using [Leaflet.VectorGrid](https://github.com/Leaflet/Leaflet.VectorGrid) plugin. You must initialize a [VectorGrid.Protobuf](https://leaflet.github.io/Leaflet.VectorGrid/vectorgrid-api-docs.html#vectorgrid-protobuf) with a URL template, just like in L.TileLayers. The difference is that you should define the styling for all the features.

```js
L.vectorGrid
  .protobuf('http://localhost:3000/points/{z}/{x}/{y}', {
    vectorTileLayerStyles: {
      'points': {
        color: 'red',
        fill: true
      }
    }
  })
  .addTo(map);
```

# Using with deck.gl

[deck.gl](https://deck.gl/) is a WebGL-powered framework for visual exploratory data analysis of large datasets.

You can add vector tiles using [MVTLayer](https://deck.gl/docs/api-reference/geo-layers/mvt-layer). MVTLayer `data` property defines the remote data for the MVT layer. It can be

- `String`: Either a URL template or a [TileJSON](https://github.com/mapbox/tilejson-spec) URL.
- `Array`: an array of URL templates. It allows to balance the requests across different tile endpoints. For example, if you define an array with 4 urls and 16 tiles need to be loaded, each endpoint is responsible to server 16/4 tiles.
- `JSON`: A valid [TileJSON object](https://github.com/mapbox/tilejson-spec/tree/master/2.2.0).

```js
const pointsLayer = new MVTLayer({
  data: 'http://localhost:3000/points', // 'http://localhost:3000/table_source/{z}/{x}/{y}'
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

# Using with Mapbox

[Mapbox GL JS](https://github.com/mapbox/mapbox-gl-js) is a JavaScript library for interactive, customizable vector maps on the web. Mapbox GL JS v1.x was open source, and it was forked as MapLibre (see [above](#using-with-maplibre)), so using Martin with Mapbox is similar to MapLibre. Mapbox GL JS can accept [MVT vector tiles](https://github.com/mapbox/vector-tile-spec) generated by Martin, and applies [a style](https://docs.mapbox.com/mapbox-gl-js/style-spec/) to them to draw a map using Web GL.

You can add a layer to the map and specify Martin TileJSON endpoint as a vector source URL. You should also specify a `source-layer` property. For [Table Sources](#table-sources) it is `{table_name}` by default.

```js
map.addLayer({
  id: 'points',
  type: 'circle',
  source: {
    type: 'vector',
    url: 'http://localhost:3000/points'
  },
  'source-layer': 'points',
  paint: {
    'circle-color': 'red'
  }
});
```

# Source List

A list of all available sources is available in a catalogue:

```shell
curl localhost:3000/catalog | jq
```

```yaml
[
  {
    "id": "function_zxy_query",
    "name": "public.function_zxy_query"
  },
  {
    "id": "points1",
    "name": "public.points1.geom"
  },
  ...
]
```

# Composite Sources

Composite Sources allows combining multiple sources into one. Composite Source consists of multiple sources separated by comma `{source1},...,{sourceN}`

Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

## Composite Source TileJSON

Composite Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{source1},...,{sourceN}`.

For example, composite source combining `points` and `lines` sources will be available at `/points,lines`

```shell
curl localhost:3000/points,lines | jq
```

## Composite Source Tiles

Composite Source tiles endpoint is available at `/{source1},...,{sourceN}/{z}/{x}/{y}`

For example, composite source combining `points` and `lines` sources will be available at `/points,lines/{z}/{x}/{y}`

```shell
curl localhost:3000/points,lines/0/0/0
```

# Table Sources

Table Source is a database table which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, Martin will go through all spatial tables in the database and build a list of table sources. A table should have at least one geometry column with non-zero SRID. All other table columns except geometry will be properties of a vector tile feature.

## Table Source TileJSON

Table Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{table_name}`.

For example, `points` table will be available at `/points`, unless there is another source with the same name, or if the table has multiple geometry columns, in which case it will be available at `/points`, `/points.1`, etc.

```shell
curl localhost:3000/points | jq
```

## Table Source Tiles

Table Source tiles endpoint is available at `/{table_name}/{z}/{x}/{y}`

For example, `points` table will be available at `/points/{z}/{x}/{y}`

```shell
curl localhost:3000/points/0/0/0
```

In case if you have multiple geometry columns in that table and want to access a particular geometry column in vector tile, you should also specify the geometry column in the table source name

```shell
curl localhost:3000/points.geom/0/0/0
```

# Function Sources

Function Source is a database function which can be used to query [vector tiles](https://github.com/mapbox/vector-tile-spec). When started, Martin will look for the functions with a suitable signature. A function that takes `z integer` (or `zoom integer`), `x integer`, `y integer`, and an optional `query json` and returns `bytea`, can be used as a Function Source. Alternatively the function could return a record with a single `bytea` field, or a record with two fields of types `bytea` and `text`, where the `text` field is an etag key (i.e. md5 hash).

| Argument                   | Type    | Description             |
|----------------------------|---------|-------------------------|
| z (or zoom)                | integer | Tile zoom parameter     |
| x                          | integer | Tile x parameter        |
| y                          | integer | Tile y parameter        |
| query (optional, any name) | json    | Query string parameters |

For example, if you have a table `table_source` in WGS84 (`4326` SRID), then you can use this function as a Function Source:

```sql, ignore
CREATE OR REPLACE FUNCTION function_zxy_query(z integer, x integer, y integer) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'function_zxy_query', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
  ) as tile WHERE geom IS NOT NULL;

  RETURN mvt;
END
$$ LANGUAGE plpgsql IMMUTABLE STRICT PARALLEL SAFE;
```

```sql, ignore
CREATE OR REPLACE FUNCTION function_zxy_query(z integer, x integer, y integer, query_params json) RETURNS bytea AS $$
DECLARE
  mvt bytea;
BEGIN
  SELECT INTO mvt ST_AsMVT(tile, 'function_zxy_query', 4096, 'geom') FROM (
    SELECT
      ST_AsMVTGeom(ST_Transform(ST_CurveToLine(geom), 3857), ST_TileEnvelope(z, x, y), 4096, 64, true) AS geom
    FROM table_source
    WHERE geom && ST_Transform(ST_TileEnvelope(z, x, y), 4326)
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
  --get localhost:3000/function_zxy_query/0/0/0
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

```sql, ignore
...WHERE answer = (query_params->'objectParam'->>'answer')::int;
```

## Function Source TileJSON

Function Source [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint is available at `/{function_name}`

For example, `points` function will be available at `/points`

```shell
curl localhost:3000/points | jq
```

## Function Source Tiles

Function Source tiles endpoint is available at `/{function_name}/{z}/{x}/{y}`

For example, `points` function will be available at `/points/{z}/{x}/{y}`

```shell
curl localhost:3000/points/0/0/0
```

# MBTile and PMTile Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files.  To serve a file from CLI, simply put the path to the file or the directory with `*.mbtiles` or `*.pmtiles` files. For example:

```shell
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory
```

You may also want to generate a [config file](#configuration-file) using the `--save-config my-config.yaml`, and later edit it and use it with `--config my-config.yaml` option.


# Command-line Interface

You can configure Martin using command-line interface. See `martin --help` or `cargo run -- --help` for more information.

```shell
Usage: martin [OPTIONS] [CONNECTION]...

Arguments:
  [CONNECTION]...  Connection strings, e.g. postgres://... or /path/to/files

Options:
  -c, --config <CONFIG>
          Path to config file. If set, no tile source-related parameters are allowed
      --save-config <SAVE_CONFIG>
          Save resulting config to a file or use "-" to print to stdout. By default, only print if sources are auto-detected
  -k, --keep-alive <KEEP_ALIVE>
          Connection keep alive timeout. [DEFAULT: 75]
  -l, --listen-addresses <LISTEN_ADDRESSES>
          The socket address to bind. [DEFAULT: 0.0.0.0:3000]
  -W, --workers <WORKERS>
          Number of web server workers
  -b, --disable-bounds
          Disable the automatic generation of bounds for spatial tables
      --ca-root-file <CA_ROOT_FILE>
          Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates
  -d, --default-srid <DEFAULT_SRID>
          If a spatial table has SRID 0, then this default SRID will be used as a fallback
  -p, --pool-size <POOL_SIZE>
          Maximum connections pool size [DEFAULT: 20]
  -m, --max-feature-count <MAX_FEATURE_COUNT>
          Limit the number of features in a tile from a PG table source
  -h, --help
          Print help
  -V, --version
          Print version
```

# Environment Variables

You can also configure Martin using environment variables, but only if the configuration file is not used. See [configuration section](#configuration-file) on how to use environment variables with config files. See also [SSL configuration](#postgresql-ssl-connections) section below.

| Environment var <br/> Config File key    | Example                              | Description                                                                                                                                                                                                                                                                                            |
|------------------------------------------|--------------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `DATABASE_URL` <br/> `connection_string` | `postgresql://postgres@localhost/db` | Postgres database connection                                                                                                                                                                                                                                                                           |
| `DEFAULT_SRID` <br/> `default_srid`      | `4326`                               | If a PostgreSQL table has a geometry column with SRID=0, use this value instead                                                                                                                                                                                                                        |
| `PGSSLCERT` <br/> `ssl_cert`             | `./postgresql.crt`                   | A file with a client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLCERT)                                                                                                                                                                         |
| `PGSSLKEY` <br/> `ssl_key`               | `./postgresql.key`                   | A file with the key for the client SSL certificate. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLKEY)                                                                                                                                                            |
| `PGSSLROOTCERT` <br/> `ssl_root_cert`    | `./root.crt`                         | A file with trusted root certificate(s). The file should contain a sequence of PEM-formatted CA certificates. [docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNECT-SSLROOTCERT)<br/>This env var used to be called `CA_ROOT_FILE`, but support for it will be removed soon. |

# Configuration File

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file. To start Martin with a configuration file you need to pass a path to a file with a `--config` argument. Config files may contain environment variables, which will be expanded before parsing. For example, to use `MY_DATABASE_URL` in your config file: `connection_string: ${MY_DATABASE_URL}`, or with a default `connection_string: ${MY_DATABASE_URL:-postgresql://postgres@localhost/db}`

```shell
martin --config config.yaml
```

You may wish to auto-generate a config file with `--save-config` argument. This will generate a config yaml file with all of your configuration, which you can edit to remove any sources you don't want to expose.

```yaml
# Connection keep alive timeout [default: 75]
keep_alive: 75

# The socket address to bind [default: 0.0.0.0:3000]
listen_addresses: '0.0.0.0:3000'

# Number of web server workers
worker_processes: 8

# Database configuration. This can also be a list of PG configs.
postgres:
  # Database connection string. You can use env vars too, for example:
  #   $DATABASE_URL
  #   ${DATABASE_URL:-postgresql://postgres@localhost/db}
  connection_string: 'postgresql://postgres@localhost:5432/db'

  # Same as PGSSLCERT for psql
  ssl_cert: './postgresql.crt'
  # Same as PGSSLKEY for psql
  ssl_key: './postgresql.key'
  # Same as PGSSLROOTCERT for psql
  ssl_root_cert: './root.crt'

  #  If a spatial table has SRID 0, then this SRID will be used as a fallback
  default_srid: 4326
  
  # Maximum connections pool size [default: 20]
  pool_size: 20

  # Limit the number of table geo features included in a tile. Unlimited by default.
  max_feature_count: 1000

  # Control the automatic generation of bounds for spatial tables [default: false]
  # If enabled, it will spend some time on startup to compute geometry bounds.
  disable_bounds: false

  # Enable automatic discovery of tables and functions. You may set this to `false` to disable.
  auto_publish:
    # Optionally limit to just these schemas
    from_schemas:
      - public
      - my_schema
    # Here we enable both tables and functions auto discovery.
    # You can also enable just one of them by not mentioning the other,
    # or setting it to false.  Setting one to true disables the other one as well.
    # E.g. `tables: false` enables just the functions auto-discovery.
    tables:
      # Optionally set a custom source ID based on the table name
      id_format: 'table.{schema}.{table}.{column}'
      # Add more schemas to the ones listed above
      from_schemas: my_other_schema
      # Optionally, publish tiles in more than one projection/tiling scheme
      tile_systems:
        # the default web mercator tiles, enabled by default 
        - type: WebMercatorQuad
        # an additional custom tiling scheme
        - type: Custom
          srid: 4326
          # bounds of tile 0/0/0
          bounds: [ -180.0, -90.0, 180.0, 90.0 ]
          # name for the tiling scheme 
          # auto published table will be given the name {source_id}:{tile_system.identifier}
          identifier: WGS84Quad
    functions:
      id_format: '{schema}.{function}'
  
  # Associative arrays of table sources
  tables:
    table_source_id:
      # ID of the MVT layer (optional, defaults to table name)
      layer_id: table_source
      
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
      bounds: [ -180.0, -90.0, 180.0, 90.0 ]
      
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
        
      # Output tiles with a custom projection and tiling
      tile_system:
        # output srid
        srid: 4326
        # bounds of tile 0/0/0
        bounds: [ -180.0, -90.0, 180.0, 90.0 ]
        # name for the tiling scheme
        identifier: WGS84Quad
  
  # Associative arrays of function sources
  functions:
    function_source_id:
      # Schema name (required)
      schema: public
      
      # Function name (required)
      function: function_zxy_query
      
      # An integer specifying the minimum zoom level
      minzoom: 0
      
      # An integer specifying the maximum zoom level. MUST be >= minzoom
      maxzoom: 30
      
      # The maximum extent of available map tiles. Bounds MUST define an area
      # covered by all zoom levels. The bounds are represented in WGS:84
      # latitude and longitude values, in the order left, bottom, right, top.
      # Values may be integers or floating point numbers.
      bounds: [ -180.0, -90.0, 180.0, 90.0 ]

# Publish PMTiles files
pmtiles:
  paths:
    # scan this whole dir, matching all *.pmtiles files
    - /dir-path
    # specific pmtiles file will be published as pmtiles2 source
    - /path/to/pmtiles.pmtiles
  sources:
    # named source matching source name to a single file
    pm-src1: /path/to/pmtiles1.pmtiles
    
# Publish MBTiles files
mbtiles:
  paths:
    # scan this whole dir, matching all *.mbtiles files
    - /dir-path
    # specific mbtiles file will be published as mbtiles2 source
    - /path/to/mbtiles.mbtiles
  sources:
    # named source matching source name to a single file
    mb-src1: /path/to/mbtiles1.mbtiles
```

# PostgreSQL Connection String
Martin supports many of the PostgreSQL connection string settings such as `host`, `port`, `user`, `password`, `dbname`, `sslmode`, `connect_timeout`, `keepalives`, `keepalives_idle`, etc. See the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING) for more details.
`
## PostgreSQL SSL Connections
Martin supports PostgreSQL `sslmode` including `disable`, `prefer`, `require`, `verify-ca` and `verify-full` modes as described in the [PostgreSQL docs](https://www.postgresql.org/docs/current/libpq-ssl.html).  Certificates can be provided in the configuration file, or can be set using the same env vars as used for `psql`. When set as env vars, they apply to all PostgreSQL connections.  See [environment vars](#environment-variables) section for more details.

By default, `sslmode` is set to `prefer` which means that SSL is used if the server supports it, but the connection is not aborted if the server does not support it.  This is the default behavior of `psql` and is the most compatible option.  Use the `sslmode` param to set a different `sslmode`, e.g. `postgresql://user:password@host/db?sslmode=require`.

# Using with Docker

You can use official Docker image [`ghcr.io/maplibre/martin`](https://ghcr.io/maplibre/martin)

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@localhost/db \
  ghcr.io/maplibre/martin
```

If you are running PostgreSQL instance on `localhost`, you have to change network settings to allow the Docker container to access the `localhost` network.

For Linux, add the `--net=host` flag to access the `localhost` PostgreSQL service.

```shell
docker run \
  --net=host \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@localhost/db \
  ghcr.io/maplibre/martin
```

For macOS, use `host.docker.internal` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@host.docker.internal/db \
  ghcr.io/maplibre/martin
```

For Windows, use `docker.for.win.localhost` as hostname to access the `localhost` PostgreSQL service.

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://postgres@docker.for.win.localhost/db \
  ghcr.io/maplibre/martin
```

# Using with Docker Compose

You can use example [`docker-compose.yml`](https://raw.githubusercontent.com/maplibre/martin/main/docker-compose.yml) file as a reference

```yml
version: '3'

services:
  martin:
    image: ghcr.io/maplibre/martin:v0.7.0
    restart: unless-stopped
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=postgresql://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:14-3.3-alpine
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

By default, Martin will be available at [localhost:3000](http://localhost:3000/)

# Using with Nginx

You can run Martin behind Nginx proxy, so you can cache frequently accessed tiles and reduce unnecessary pressure on the database.

```yml
version: '3'

services:
  nginx:
    image: nginx:alpine
    restart: unless-stopped
    ports:
      - "80:80"
    volumes:
      - ./cache:/var/cache/nginx
      - ./nginx.conf:/etc/nginx/nginx.conf:ro
    depends_on:
      - martin

  martin:
    image: maplibre/martin:v0.7.0
    restart: unless-stopped
    environment:
      - DATABASE_URL=postgresql://postgres:password@db/db
    depends_on:
      - db

  db:
    image: postgis/postgis:14-3.3-alpine
    restart: unless-stopped
    environment:
      - POSTGRES_DB=db
      - POSTGRES_USER=postgres
      - POSTGRES_PASSWORD=password
    volumes:
      - ./pg_data:/var/lib/postgresql/data
```

You can find an example Nginx configuration file [here](https://github.com/maplibre/martin/blob/main/nginx.conf).

## Rewriting URLs

If you are running Martin behind Nginx proxy, you may want to rewrite the request URL to properly handle tile URLs in [TileJSON](#table-source-tilejson) [endpoints](#function-source-tilejson).

```nginx
location ~ /tiles/(?<fwd_path>.*) {
    proxy_set_header  X-Rewrite-URL $uri;
    proxy_set_header  X-Forwarded-Host $host:$server_port;
    proxy_set_header  X-Forwarded-Proto $scheme;
    proxy_redirect    off;

    proxy_pass        http://martin:3000/$fwd_path$is_args$args;
}
```

## Caching tiles

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

# Building from Source

You can clone the repository and build Martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

```shell
git clone git@github.com:maplibre/martin.git
cd martin
cargo build --release
```

The binary will be available at `./target/release/martin`.

```shell
cd ./target/release/
./martin postgresql://postgres@localhost/db
```

# Debugging

Log levels are controlled on a per-module basis, and by default all logging is disabled except for errors. Logging is controlled via the `RUST_LOG` environment variable. The value of this environment variable is a comma-separated list of logging directives.

This will enable debug logging for all modules:

```shell
export RUST_LOG=debug
martin postgresql://postgres@localhost/db
```

While this will only enable verbose logging for the `actix_web` module and enable debug logging for the `martin` and `tokio_postgres` modules:

```shell
export RUST_LOG=actix_web=info,martin=debug,tokio_postgres=debug
martin postgresql://postgres@localhost/db
```

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

## Other useful commands

```shell
 Start db service
just debug-page

 Run Martin server
DATABASE_URL=postgresql://postgres@localhost/db cargo run
```

Open `tests/debug.html` for debugging. By default, Martin will be available at [localhost:3000](http://localhost:3000/)

Make your changes, and check if all the tests are running

```shell
DATABASE_URL=postgresql://postgres@localhost/db cargo test
```

You can also run benchmarks with

```shell
DATABASE_URL=postgresql://postgres@localhost/db cargo bench
```

An HTML report displaying the results of the benchmark will be generated under `target/criterion/report/index.html`

# Recipes
## Using with DigitalOcean PostgreSQL

You can use Martin with [Managed PostgreSQL from DigitalOcean](https://www.digitalocean.com/products/managed-databases-postgresql/) with PostGIS extension

First, you need to download the CA certificate and get your cluster connection string from the [dashboard](https://cloud.digitalocean.com/databases). After that, you can use the connection string and the CA certificate to connect to the database

```shell
martin --ca-root-file ./ca-certificate.crt postgresql://user:password@host:port/db?sslmode=require
```

## Using with Heroku PostgreSQL

You can use Martin with [Managed PostgreSQL from Heroku](https://www.heroku.com/postgres) with PostGIS extension

```shell
heroku pg:psql -a APP_NAME -c 'create extension postgis'
```

Use the same environment variables as Heroku [suggests for psql](https://devcenter.heroku.com/articles/heroku-postgres-via-mtls#step-2-configure-environment-variables).

```shell
export DATABASE_URL=$(heroku config:get DATABASE_URL -a APP_NAME)
export PGSSLCERT=DIRECTORY/PREFIXpostgresql.crt
export PGSSLKEY=DIRECTORY/PREFIXpostgresql.key
export PGSSLROOTCERT=DIRECTORY/PREFIXroot.crt

martin
```

You may also be able to validate SSL certificate with an explicit sslmode, e.g.
```shell
export DATABASE_URL="$(heroku config:get DATABASE_URL -a APP_NAME)?sslmode=verify-ca"
```
