<h1>Martin</h1>

[![Slack chat](https://img.shields.io/badge/Chat-on%20Slack-brightgreen)](https://slack.openstreetmap.us/)
[![CI](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin/actions)
![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)  
  
Martin is a tile server able to generate [vector tiles](https://github.com/mapbox/vector-tile-spec) from large [PostGIS](https://github.com/postgis/postgis) databases on the fly, or serve tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) and [MBTile](https://github.com/mapbox/mbtiles-spec) files. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

## Requirements

Martin requires PostGIS 3.0+.  PostGIS 3.1+ is recommended.

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
       -e DATABASE_URL=postgresql://user@host:port/database \
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

## Usage

### PostGIS sources
Martin requires at least one PostgreSQL [connection string](https://maplibre.org/martin/PostgreSQL-Connection-String.html) or a [tile source file](https://maplibre.org/martin/MBTile-and-PMTile-Sources.html) as a command-line argument. A PG connection string can also be passed via the `DATABASE_URL` environment variable.

```shell
martin postgresql://postgres@localhost/db
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

## Using with clients
### Using with MapLibre
[MapLibre](https://maplibre.org/projects/maplibre-gl-js/) is an Open-source JavaScript library for showing maps on a website. MapLibre can accept [MVT vector tiles](https://github.com/mapbox/vector-tile-spec) generated by Martin, and applies [a style](https://maplibre.org/maplibre-gl-js-docs/style-spec/) to them to draw a map using Web GL.

You can add a layer to the map and specify Martin [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint as a vector source URL. You should also specify a `source-layer` property. For [Table Sources](https://maplibre.org/martin/table-sources.html#table-sources) it is `{table_name}` by default.


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

You can also combine multiple sources into one source with [Composite Sources](https://maplibre.org/martin/composite-sources.html#composite-sources). Each source in a composite source can be accessed with its `{source_name}` as a `source-layer` property.

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

### Using with Leaflet

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

### Using with deck.gl

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

### Using with Mapbox

[Mapbox GL JS](https://github.com/mapbox/mapbox-gl-js) is a JavaScript library for interactive, customizable vector maps on the web. Mapbox GL JS v1.x was open source, and it was forked as MapLibre (see [above](#using-with-maplibre)), so using Martin with Mapbox is similar to MapLibre. Mapbox GL JS can accept [MVT vector tiles](https://github.com/mapbox/vector-tile-spec) generated by Martin, and applies [a style](https://docs.mapbox.com/mapbox-gl-js/style-spec/) to them to draw a map using Web GL.

You can add a layer to the map and specify Martin TileJSON endpoint as a vector source URL. You should also specify a `source-layer` property. For [Table Sources](https://maplibre.org/martin/table-sources.html#table-sources) it is `{table_name}` by default.

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
