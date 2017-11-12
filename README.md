# Falcon

[![Build Status](https://travis-ci.org/stepankuzmin/falcon.svg?branch=master)](https://travis-ci.org/stepankuzmin/falcon)

PostgreSQL RESTful API

**Warning: this is experimental**

## Installation

    git clone git@github.com:stepankuzmin/falcon.git
    cd falcon
    cargo build --release
    ./target/release/falcon

## Usage

    DATABASE_URL=postgres://postgres:password@localhost:5432/test falcon

## Using with Docker

    docker run -d —rm —name falcon \
      -p 3000:3000 \
      -e DATABASE_URL=postgres://postgres:password@localhost:5432/test \
      stepankuzmin/falcon

## Development

Install project dependencies and check that the tests run

    cargo test
    cargo run
