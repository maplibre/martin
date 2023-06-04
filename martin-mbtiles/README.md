# martin-mbtiles

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin/tools.html)
[![docs.rs docs](https://docs.rs/martin-mbtiles/badge.svg)](https://docs.rs/martin-mbtiles)
[![Slack chat](https://img.shields.io/badge/Chat-on%20Slack-blueviolet)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin-mbtiles.svg)](https://crates.io/crates/martin-mbtiles)
[![CI build](https://github.com/maplibre/martin/workflows/CI/badge.svg)](https://github.com/maplibre/martin-mbtiles/actions)

A library to help tile servers like [Martin](https://maplibre.org/martin) work with [MBTiles](https://github.com/mapbox/mbtiles-spec) files.

This crate also has a small utility that allows users to interact with the `*.mbtiles` files from the command line.  See [tools](https://maplibre.org/martin/tools.html) documentation for more information.

### Development

Any changes to SQL commands require running of `just prepare-sqlite`.  This will install `cargo sqlx` command if it is not already installed, and update the `./sqlx-data.json` file.
