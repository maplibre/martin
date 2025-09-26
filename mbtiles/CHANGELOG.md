# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.0](https://github.com/maplibre/martin/compare/mbtiles-v0.12.2...mbtiles-v0.13.0) - 2025-09-26

### Added

- *(mbtiles)* export `martin_tile_utils::{Tile, TileCoord}` publicly ([#2025](https://github.com/maplibre/martin/pull/2025))

### Fixed

- fix clippy::ignore-without-reason (v2) ([#1904](https://github.com/maplibre/martin/pull/1904))
- clippy::ignore-without-reason ([#1903](https://github.com/maplibre/martin/pull/1903))

### Other

- *(release)* improve release-plz config ([#2242](https://github.com/maplibre/martin/pull/2242))
- *(release)* bump pmtiles ([#2232](https://github.com/maplibre/martin/pull/2232))
- *(ci)* fix clippy::collapsible-if ([#2233](https://github.com/maplibre/martin/pull/2233))
- *(core)* more consitently use `#[non_exhaustive]` and `#[source]` in our public `thiserror` errors ([#2217](https://github.com/maplibre/martin/pull/2217))
- use let-if chains ([#2052](https://github.com/maplibre/martin/pull/2052))
- *(ci)* add cargo-sort to consistently sort our `Cargo.toml` ([#2020](https://github.com/maplibre/martin/pull/2020))
- move from eslint to biomejs for formatting/linting ([#1909](https://github.com/maplibre/martin/pull/1909))
- *(core)* move intial part of the tile catalog to be `martin_core` (3/n) ([#2049](https://github.com/maplibre/martin/pull/2049))
