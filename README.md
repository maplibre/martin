# Martin

![CircleCI](https://img.shields.io/circleci/project/github/urbica/martin.svg?style=popout)

Martin is a PostGIS [Mapbox Vector Tiles](https://github.com/mapbox/vector-tile-spec) server written in Rust using [Actix](https://github.com/actix/actix-web) web framework.

## Installation

You can download Martin from the [Github releases page](https://github.com/urbica/martin/releases).

If you are running macOS and use [Homebrew](https://brew.sh/), you can install martin using Homebrew tap.

```shell
brew tap urbica/tap
brew install martin
```

## Usage

Martin requires a database connection string. It can be passed as a command-line argument or as a `DATABASE_URL` environment variable.

```shell
martin postgres://postgres@localhost/db
```

### Table Sources List

Table Sources list endpoint is available at `/index.json`

```shell
curl localhost:3000/index.json
```

### Table Source TileJSON

Table Source TileJSON endpoint is available at `/{schema_name}.{table_name}.json`.

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

### Function Sources List

Function Sources list endpoint is available at `/rpc/index.json`

```shell
curl localhost:3000/rpc/index.json
```

### Function Source TileJSON

Function Source TileJSON endpoint is available at `/rpc/{schema_name}.{function_name}.json`

For example, `points` function in `public` schema will be available at `/rpc/public.points.json`

```shell
curl localhost:3000/rpc/public.points.json
```

### Function Source tiles

Function Source tiles endpoint is available at `/rpc/{schema_name}.{function_name}/{z}/{x}/{y}.pbf`

For example, `points` function in `public` schema will be available at `/rpc/public.points/{z}/{x}/{y}.pbf`

```shell
curl localhost:3000/rpc/public.points/0/0/0.pbf
```

## Using with Mapbox GL JS

[Mapbox GL JS](https://github.com/mapbox/mapbox-gl-js) is a JavaScript library for interactive, customizable vector maps on the web. It takes map styles that conform to the
[Mapbox Style Specification](https://www.mapbox.com/mapbox-gl-js/style-spec), applies them to vector tiles that
conform to the [Mapbox Vector Tile Specification](https://github.com/mapbox/vector-tile-spec), and renders them using
WebGL.

You can add a layer to the map and specify martin TileJSON endpoint as a vector source url. You should also specify a `source-layer` property. For Table Sources it is `{schema_name}.{table_name}` by default.

```js
map.addLayer({
  "id": "public.points",
  "type": "circle",
  "source": {
    "type": "vector",
    "url": "http://localhost:3000/public.points.json",
  },
  "source-layer": "public.points"
});
```

## Command-line interface

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

## Environment variables

You can also configure martin using environment variables

| Environment variable | Example                          | Description                   |
|----------------------|----------------------------------|-------------------------------|
| DATABASE_URL         | postgres://postgres@localhost/db | postgres database connection  |
| DATABASE_POOL_SIZE   | 20                               | maximum connections pool size |
| WORKER_PROCESSES     | 8                                | number of web server workers  |
| KEEP_ALIVE           | 75                               | connection keep alive timeout |

## Using with Docker

You can use official Docker image `urbica/martin`

```shell
docker run \
  -p 3000:3000 \
  -e DATABASE_URL=postgres://postgres@localhost/db \
  urbica/martin
```

## Building from source

You can clone repository and build martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

```shell
git clone git@github.com:urbica/martin.git
cd martin
cargo build --release
```

The binary will be available at `./target/release/martin`

```shell
cd ./target/release/
./martin postgres://postgres@localhost/db
```

## Development

Install project dependencies and check that all the tests run

```shell
cargo test
cargo run
```