# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
