[![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)](https://maplibre.org/martin/)

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin)
[![docs.rs docs](https://docs.rs/martin/badge.svg)](https://docs.rs/martin)
[![](https://img.shields.io/badge/Slack-%23maplibre--martin-blueviolet?logo=slack)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin.svg)](https://crates.io/crates/martin)
[![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)](https://github.com/maplibre/martin/security)
[![CI build](https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg)](https://github.com/maplibre/martin/actions)

Martin is a tile server and a set of tools able to generate vector tiles on the fly
from large PostgreSQL databases, and serve tiles from PMTiles and MBTiles files. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

## Features

* Serve [vector tiles](https://github.com/mapbox/vector-tile-spec) from
    * [PostGIS](https://github.com/postgis/postgis) databases, automatically discovering compatible tables and functions
    * [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new), both local files and over HTTP
    * [MBTile](https://github.com/mapbox/mbtiles-spec) files
* [Combine](https://maplibre.org/martin/sources-composite.html) multiple tile sources into one
* Generate [sprites](https://maplibre.org/martin/sources-sprites.html) and [font glyphs](https://maplibre.org/martin/sources-fonts.html)
* Generate tiles in bulk from any Martin-supported sources into an MBTiles file with [martin-cp](https://maplibre.org/martin/martin-cp.html) tool
* Examine, copy, validate, compare, and apply diffs between MBTiles files with [mbtiles](https://maplibre.org/martin/tools.html#mbtiles) tool

## Documentation

* [Quick Start](https://maplibre.org/martin/quick-start.html)
* [Installation](https://maplibre.org/martin/installation.html)
* Running with [CLI](https://maplibre.org/martin/run-with-cli.html)
  or [configuration file](https://maplibre.org/martin/config-file.html)
* [Usage and API](https://maplibre.org/martin/using.html)

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)
  at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
