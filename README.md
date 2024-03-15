[![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)](https://maplibre.org/martin/)

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin)
[![docs.rs docs](https://docs.rs/martin/badge.svg)](https://docs.rs/martin)
[![](https://img.shields.io/badge/Slack-%23maplibre--martin-blueviolet?logo=slack)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin.svg)](https://crates.io/crates/martin)
[![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)](https://github.com/maplibre/martin/security)
[![CI build](https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg)](https://github.com/maplibre/martin/actions)

Martin is a tile server able to generate and serve [vector tiles](https://github.com/mapbox/vector-tile-spec) on the fly from large [PostGIS](https://github.com/postgis/postgis) databases, [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) (local or remote), and [MBTile](https://github.com/mapbox/mbtiles-spec) files, allowing multiple tile sources to be dynamically combined into one. Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

Additionally, there are [several tools](https://maplibre.org/martin/tools.html) for generating tiles in bulk from any Martin-supported sources (similar to `tilelive-copy`), copying tiles between MBTiles files, creating deltas (patches) and applying them, and validating MBTiles files.

See [Martin book](https://maplibre.org/martin/) for complete documentation.

## Re-use Martin as a library

Martin can be used as a standalone server, or as a library in your own Rust application. When used as a library, you can use the following features:

* **postgres** - enable PostgreSQL/PostGIS tile sources
* **pmtiles** - enable PMTile tile sources
* **mbtiles** - enable MBTile tile sources
* **fonts** - enable font sources
* **sprites** - enable sprite sources

## Documentation

See [Martin book](https://maplibre.org/martin/) for complete documentation.

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
