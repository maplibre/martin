# Martin

![CircleCI](https://img.shields.io/circleci/project/github/urbica/martin.svg?style=popout)

Martin is a PostGIS [Mapbox Vector Tiles](https://github.com/mapbox/vector-tile-spec) server written in Rust using [Actix](https://github.com/actix/actix-web) web framework.

## Installation

You can download Martin from the [Github releases page](https://github.com/urbica/martin/releases).

If you are running macOS and use [Homebrew](https://brew.sh/), you can install Martin using Homebrew tap.

```shell
brew tap urbica/tap
brew install martin
```

## Usage

Martin requires a database connection string. It can be passed as a command-line argument or as a `DATABASE_URL` environment variable.

```shell
martin postgres://postgres@localhost/db
```

Martin also supports a list of options

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

You can configure martin with environment variables

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

You can clone repository and build Martin using [cargo](https://doc.rust-lang.org/cargo) package manager.

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