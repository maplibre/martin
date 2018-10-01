# Martin

[![Build Status](https://travis-ci.org/urbica/martin.svg?branch=master)](https://travis-ci.org/urbica/martin)

Martin is a PostGIS [Mapbox Vector Tiles](https://github.com/mapbox/vector-tile-spec) server written in Rust using [Actix](https://github.com/actix/actix-web) web framework.

**Warning: this is experimental**

## Installation

    git clone git@github.com:urbica/martin.git
    cd martin
    cargo build --release
    ./target/release/martin

## Usage

    DATABASE_URL=postgres://postgres:password@localhost:5432/test martin

## Environment variables

    DATABASE_URL
    DATABASE_POOL_SIZE
    WORKER_PROCESSES
    KEEP_ALIVE

## Using with Docker

    docker run -d --rm --name postgres \
      -p 5432:5432 \
      -e POSTGRES_PASSWORD=password \
      mdillon/postgis:10-alpine

    docker run -d --rm --name martin \
      -p 3000:3000 \
      -e DATABASE_URL=postgres://postgres:password@localhost:5432/test \
      urbica/martin

## Development

Install project dependencies and check that the tests run

    cargo test
    cargo run
