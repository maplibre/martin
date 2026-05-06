# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.17.4](https://github.com/maplibre/martin/compare/mbtiles-v0.17.3...mbtiles-v0.17.4) - 2026-05-06

### Added

- mvt->mlt pre-processing encoding ([#2769](https://github.com/maplibre/martin/pull/2769))

### Other

- *(mbtiles)* migrate the mbtiles crate to structured logs ([#2778](https://github.com/maplibre/martin/pull/2778))

## [0.17.3](https://github.com/maplibre/martin/compare/mbtiles-v0.17.2...mbtiles-v0.17.3) - 2026-04-29

### Other

- *(mbtiles)* migrate from log/env_logger to tracing ([#2755](https://github.com/maplibre/martin/pull/2755))

## [0.17.2](https://github.com/maplibre/martin/compare/mbtiles-v0.17.1...mbtiles-v0.17.2) - 2026-04-29

### Other

- update Cargo.toml dependencies

## [0.17.1](https://github.com/maplibre/martin/compare/mbtiles-v0.17.0...mbtiles-v0.17.1) - 2026-04-28

### Added

- support `content_type` in PostgreSQL function source SQL comments for raster tiles ([#2671](https://github.com/maplibre/martin/pull/2671))

## [0.17.0](https://github.com/maplibre/martin/compare/mbtiles-v0.16.0...mbtiles-v0.17.0) - 2026-04-23

### Added

- *(mbtiles)* add --strict flag to use STRICT SQLite tables ([#2712](https://github.com/maplibre/martin/pull/2712))

## [0.16.0](https://github.com/maplibre/martin/compare/mbtiles-v0.15.4...mbtiles-v0.16.0) - 2026-04-18

### Added

- store compression type in the MBTiles metadata table ([#2618](https://github.com/maplibre/martin/pull/2618))
- *(mbtiles)* Add a transcoder API ([#2682](https://github.com/maplibre/martin/pull/2682))
- *(mbtiles)* Support planetilers' normalised schema ([#2681](https://github.com/maplibre/martin/pull/2681))

### Other

- Enable `clippy::unwrap_used` workspace lint ([#2670](https://github.com/maplibre/martin/pull/2670))
- hotpath based profiling integration ([#2663](https://github.com/maplibre/martin/pull/2663))
- impl the accept header ([#2703](https://github.com/maplibre/martin/pull/2703))

## [0.15.4](https://github.com/maplibre/martin/compare/mbtiles-v0.15.3...mbtiles-v0.15.4) - 2026-04-02

### Fixed

- typos ([#2651](https://github.com/maplibre/martin/pull/2651))

### Other

- Enable `clippy::use_self` at workspace level and resolve all violations ([#2645](https://github.com/maplibre/martin/pull/2645))

## [0.15.3](https://github.com/maplibre/martin/compare/mbtiles-v0.15.2...mbtiles-v0.15.3) - 2026-03-14

### Added

- *(srv)* resolve some compression TODOS which adds zstd support ([#2597](https://github.com/maplibre/martin/pull/2597))

### Fixed

- Accept any INT-containing type in MBTiles validation to be an integer ([#2560](https://github.com/maplibre/martin/pull/2560))

### Other

- rename the `webp.sql` fixture to `webp-no-primary.sql` ([#2564](https://github.com/maplibre/martin/pull/2564))
- More restrictive expects ([#2562](https://github.com/maplibre/martin/pull/2562))

## [0.15.2](https://github.com/maplibre/martin/compare/mbtiles-v0.15.1...mbtiles-v0.15.2) - 2026-02-11

### Other

- restrict `unused_trait_names` for trait imports ([#2542](https://github.com/maplibre/martin/pull/2542))

## [0.15.1](https://github.com/maplibre/martin/compare/mbtiles-v0.15.0...mbtiles-v0.15.1) - 2026-01-27

### Added

- add MLT decoding support ([#2512](https://github.com/maplibre/martin/pull/2512))
- migrate our `log` library to `tracing`. For practical use cases this does not have an effect, since we enable the `log` feature by default ([#2494](https://github.com/maplibre/martin/pull/2494))

### Other

- *(test)* unignore `diff_and_patch_bsdiff` test with unique SQLite database names ([#2480](https://github.com/maplibre/martin/pull/2480))
- *(test)* remove the prefix-ism around how files are named for binary diff copy and simplify their naming ([#2478](https://github.com/maplibre/martin/pull/2478))
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
- update a variety of likely uncritical dependencies ([#2471](https://github.com/maplibre/martin/pull/2471))

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

- *(lints)* audit all allows, add reasons and remove unnecessary ones ([#2288](https://github.com/maplibre/martin/pull/2288))
- *(config)* [**breaking**] remove deprecated `MbtilesPool::new` ([#2294](https://github.com/maplibre/martin/pull/2294))
- *(lints)* migrate a few of our expects to unwraps ([#2284](https://github.com/maplibre/martin/pull/2284))
- *(lints)* apply `clippy::panic_in_result_fn` and `clippy::todo` as warnings ([#2283](https://github.com/maplibre/martin/pull/2283))
- *(mbtiles)* Generate mbtiles dynamically from SQL files to increase debuggability and transparency ([#1868](https://github.com/maplibre/martin/pull/1868))

## [0.13.1](https://github.com/maplibre/martin/compare/mbtiles-v0.13.0...mbtiles-v0.13.1) - 2025-09-28

### Other

- The previous `martin-0.19.1` release had another bug which prevented proper releasing which is now fixed ([#2262](https://github.com/maplibre/martin/pull/2262))
- update Cargo.lock dependencies

## [0.13.0](https://github.com/maplibre/martin/compare/mbtiles-v0.12.2...mbtiles-v0.13.0) - 2025-09-26

### Breaking Changes

- *(core)* More consistently use `#[non_exhaustive]` and `#[source]` in our public `thiserror` errors ([#2217](https://github.com/maplibre/martin/pull/2217))

### Added

- *(lib)* export `martin_tile_utils::{Tile, TileCoord}` publicly ([#2025](https://github.com/maplibre/martin/pull/2025))

### Other

- *(release)* improve release-plz config ([#2242](https://github.com/maplibre/martin/pull/2242))
- *(ci)* add cargo-sort to consistently sort our `Cargo.toml` ([#2020](https://github.com/maplibre/martin/pull/2020))
- fix clippy and other code style related issues ([#1904](https://github.com/maplibre/martin/pull/1904), [#1903](https://github.com/maplibre/martin/pull/1903), [#1909](https://github.com/maplibre/martin/pull/1909), [#2052](https://github.com/maplibre/martin/pull/2052), [#2233](https://github.com/maplibre/martin/pull/2233))
