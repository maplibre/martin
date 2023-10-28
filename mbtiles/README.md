# mbtiles

[![Book](https://img.shields.io/badge/docs-Book-informational)](https://maplibre.org/martin/50-tools.html)
[![docs.rs docs](https://docs.rs/mbtiles/badge.svg)](https://docs.rs/mbtiles)
[![Slack chat](https://img.shields.io/badge/Chat-on%20Slack-blueviolet)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/mbtiles.svg)](https://crates.io/crates/mbtiles)
[![CI build](https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg)](https://github.com/maplibre/martin/actions)

A library to help tile servers like [Martin](https://maplibre.org/martin) work with [MBTiles](https://github.com/mapbox/mbtiles-spec) files. When using as a lib, you may want to disable default features (i.e. the unused "cli" feature).

This crate also has a small utility that allows users to interact with the `*.mbtiles` files from the command line.  See [tools](https://maplibre.org/martin/50-tools.html) documentation for more information.

### Development

Any changes to SQL commands require running of `just prepare-sqlite`.  This will install `cargo sqlx` command if it is not already installed, and update the `./sqlx-data.json` file.

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
