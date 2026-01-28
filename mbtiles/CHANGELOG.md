# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.15.1](https://github.com/maplibre/martin/compare/mbtiles-v0.15.0...mbtiles-v0.15.1) - 2026-01-27

### Added

- add MLT decoding support ([#2512](https://github.com/maplibre/martin/pull/2512))
- migrate our `log` library to `tracing`. For practical use cases this does not have an effect, since we enable the `log` feature by default ([#2494](https://github.com/maplibre/martin/pull/2494))

### Other

- *(test)* unignore `diff_and_patch_bsdiff` test with unique SQLite database names ([#2480](https://github.com/maplibre/martin/pull/2480))
- *(test)* remove the prefix-ism around how files are named for binary diff copy and simpify their naming ([#2478](https://github.com/maplibre/martin/pull/2478))
- *(test)* add assertion messages what we are checking to the copy tests ([#2477](https://github.com/maplibre/martin/pull/2477))

## [0.15.0](https://github.com/maplibre/martin/compare/mbtiles-v0.14.3...mbtiles-v0.15.0) - 2026-01-03

### Added

Configurable output formats for `mbtiles summary`.
You can now control the output format using the `--format` option:
- `json-pretty` — multi-line, human-readable JSON
- `json` — compact JSON
- `text` — plain text (default)

Implemented in [#2447](https://github.com/maplibre/martin/pull/2447) by @nyurik.

### Fixed handling of empty MBTiles archives in `mbtiles meta-all`

The command now:
- Separates metadata from the detected tile format
- Treats missing metadata as an error when creating a source for compatibility
- Outputs `null` metadata when the archive is empty

Fixed in [#2448](https://github.com/maplibre/martin/pull/2448) by @nyurik.

### Other

- *(mbtiles)* Generate mbtiles dynamically from SQL files to increase debuggability and transparency ([#2380](https://github.com/maplibre/martin/pull/2380))
- update a variety of likely uncritical dependencys ([#2471](https://github.com/maplibre/martin/pull/2471))

## [0.14.3](https://github.com/maplibre/martin/compare/mbtiles-v0.14.2...mbtiles-v0.14.3) - 2025-12-11

### Other

- update Cargo.lock dependencies

## [0.14.2](https://github.com/maplibre/martin/compare/mbtiles-v0.14.1...mbtiles-v0.14.2) - 2025-11-07

### Fixed

- fix assertion ([#2340](https://github.com/maplibre/martin/pull/2340))

## [0.14.1](https://github.com/maplibre/martin/compare/mbtiles-v0.14.0...mbtiles-v0.14.1) - 2025-11-03

### Other

- *(mbtiles)* Improve/Extend a large part of the doc comments ([#2334](https://github.com/maplibre/martin/pull/2334))

## [0.14.0](https://github.com/maplibre/martin/compare/mbtiles-v0.13.1...mbtiles-v0.14.0) - 2025-10-27

### Other

- *(lints)* audit all allows, add reasons and remove unnessesary ones ([#2288](https://github.com/maplibre/martin/pull/2288))
- *(config)* [**breaking**] remove deprecated `MbtilesPool::new` ([#2294](https://github.com/maplibre/martin/pull/2294))
- *(lints)* migrate a few of our expects to unwraps ([#2284](https://github.com/maplibre/martin/pull/2284))
- *(lints)* applly `clippy::panic_in_result_fn` and `clippy::todo` as warnings ([#2283](https://github.com/maplibre/martin/pull/2283))
- *(mbtiles)* Generate mbtiles dynamically from SQL files to increase debuggability and transparency ([#1868](https://github.com/maplibre/martin/pull/1868))

## [0.13.1](https://github.com/maplibre/martin/compare/mbtiles-v0.13.0...mbtiles-v0.13.1) - 2025-09-28

### Other

- The previous `martin-0.19.1` release had another bug which prevented proper releasing which is now fixed ([#2262](https://github.com/maplibre/martin/pull/2262))
- update Cargo.lock dependencies

## [0.13.0](https://github.com/maplibre/martin/compare/mbtiles-v0.12.2...mbtiles-v0.13.0) - 2025-09-26

### Breaking Changes

- *(core)* More consitently use `#[non_exhaustive]` and `#[source]` in our public `thiserror` errors ([#2217](https://github.com/maplibre/martin/pull/2217))

### Added

- *(lib)* export `martin_tile_utils::{Tile, TileCoord}` publicly ([#2025](https://github.com/maplibre/martin/pull/2025))

### Other

- *(release)* improve release-plz config ([#2242](https://github.com/maplibre/martin/pull/2242))
- *(ci)* add cargo-sort to consistently sort our `Cargo.toml` ([#2020](https://github.com/maplibre/martin/pull/2020))
- fix clippy and other code style related issues ([#1904](https://github.com/maplibre/martin/pull/1904), [#1903](https://github.com/maplibre/martin/pull/1903), [#1909](https://github.com/maplibre/martin/pull/1909), [#2052](https://github.com/maplibre/martin/pull/2052), [#2233](https://github.com/maplibre/martin/pull/2233))
