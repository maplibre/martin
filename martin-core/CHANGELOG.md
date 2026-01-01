# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.5](https://github.com/maplibre/martin/compare/martin-core-v0.2.4...martin-core-v0.2.5) - 2025-12-27

Updated dependencies

## [0.2.4](https://github.com/maplibre/martin/compare/martin-core-v0.2.3...martin-core-v0.2.4) - 2025-12-11

### Added

- *(martin-core)* add an pmtiles example ([#2370](https://github.com/maplibre/martin/pull/2370))

## [0.2.3](https://github.com/maplibre/martin/compare/martin-core-v0.2.2...martin-core-v0.2.3) - 2025-11-18

A recent release of pmtiles had a breaking change.
This release migrates to the new usage.

## [0.2.2](https://github.com/maplibre/martin/compare/martin-core-v0.2.1...martin-core-v0.2.2) - 2025-11-07

### Other

- updated the following local packages: martin-tile-utils, mbtiles

## [0.2.1](https://github.com/maplibre/martin/compare/martin-core-v0.2.0...martin-core-v0.2.1) - 2025-11-03

### Other

- updated the following local packages: mbtiles

## [0.2.0](https://github.com/maplibre/martin/compare/martin-core-v0.1.3...martin-core-v0.2.0) - 2025-10-27

### Added

- unstable style rendering ([#2306](https://github.com/maplibre/martin/pull/2306))
- *(cache)* implement sprite caching ([#2295](https://github.com/maplibre/martin/pull/2295))
- add font caching ([#2304](https://github.com/maplibre/martin/pull/2304))
- *(cache)* [**breaking**] split the cache configuration of tiles and pmtiles directories ([#2303](https://github.com/maplibre/martin/pull/2303))
- *(core)* enable overriding of the automatic hashing for source traits ([#2293](https://github.com/maplibre/martin/pull/2293))
- *(pmtiles)* [**breaking**] change pmtiles to base the implementation on `object_storage` instead ([#2251](https://github.com/maplibre/martin/pull/2251))

### Fixed

- *(cog)* [**breaking**] rename `cog` feature to `unstable-cog` ([#2285](https://github.com/maplibre/martin/pull/2285))

### Other

- *(lints)* audit all allows, add reasons and remove unnessesary ones ([#2288](https://github.com/maplibre/martin/pull/2288))
- *(core)* add a `_tiles` feature  to simplify our feature configuration ([#2296](https://github.com/maplibre/martin/pull/2296))
- move `MainCache` to be a `TileCache` ([#2297](https://github.com/maplibre/martin/pull/2297))
- *(lints)* migrate a few of our expects to unwraps ([#2284](https://github.com/maplibre/martin/pull/2284))

## [0.1.3](https://github.com/maplibre/martin/compare/martin-core-v0.1.2...martin-core-v0.1.3) - 2025-10-01

### Added

- `Source::get_version` which returns the version of pmtiles sources allowing for better caching in some circumstances ([#2198](https://github.com/maplibre/martin/pull/2198))

### Other

- release ([#2265](https://github.com/maplibre/martin/pull/2265))

## [0.1.2](https://github.com/maplibre/martin/compare/martin-core-v0.1.1...martin-core-v0.1.2) - 2025-09-28

### Other

- The previous `martin-0.19.1` release had another bug which prevented proper releasing which is now fixed ([#2262](https://github.com/maplibre/martin/pull/2262))
- updated the following local packages: mbtiles

## [0.1.1](https://github.com/maplibre/martin/compare/martin-core-v0.1.0...martin-core-v0.1.1) - 2025-09-27

- fix release not working for some packages due to outdated dependedncy definitions
- update documentation to reflect the features better

## [0.1.0](https://github.com/maplibre/martin/releases/tag/martin-core-v0.1.0) - 2025-09-26

This marks the v0.1 relese, where we moved over the largest part of the previous `martin` crate.
The motivation for this split is mostly to be able to not couple the SemVer promise for `martin` and `martin-core`, i.e. just because something gets refactored in `martin`, `martin-core` has a breaking release without breakage.

### Other

- *(release)* bump pmtiles ([#2232](https://github.com/maplibre/martin/pull/2232))
- *(core)* remove the last bit of the onetime use utils ([#2227](https://github.com/maplibre/martin/pull/2227))
- *(ci)* Split tests and lints in CI ([#2225](https://github.com/maplibre/martin/pull/2225))
- *(core)* be consistent in posgres naming ([#2215](https://github.com/maplibre/martin/pull/2215))
- *(core)* more consitently use `#[non_exhaustive]` and `#[source]` in our public `thiserror` errors ([#2217](https://github.com/maplibre/martin/pull/2217))
- *(core)* move error types to more appropriate places ([#2213](https://github.com/maplibre/martin/pull/2213))
- *(core)* fix MartinCoreError being a `Box<dyn Error>` ([#2216](https://github.com/maplibre/martin/pull/2216))
- *(core)* minimise the dependedncy Postgres needs for both core and non-core ([#2194](https://github.com/maplibre/martin/pull/2194))
- apply the no `use super::..` execept in tests guidance ([#2193](https://github.com/maplibre/martin/pull/2193))
- *(core)* move postgres' `PgPool` and `PgSource` to the core ([#2191](https://github.com/maplibre/martin/pull/2191))
- *(core)* move config handling out of the pool ([#2185](https://github.com/maplibre/martin/pull/2185))
- *(core)* move pmtiles to the new location ([#2182](https://github.com/maplibre/martin/pull/2182))
- *(core)* move the postgres errors to the core ([#2184](https://github.com/maplibre/martin/pull/2184))
- *(core)* move the cache to the core ([#2179](https://github.com/maplibre/martin/pull/2179))
- *(core)* move the cog source to the core ([#2172](https://github.com/maplibre/martin/pull/2172))
- *(core)* migrate mbtiles to the core ([#2171](https://github.com/maplibre/martin/pull/2171))
- *(core)* move `Source` to the core ([#2167](https://github.com/maplibre/martin/pull/2167))
- *(core)* change the errors of the Source trait to be dyn based ([#2158](https://github.com/maplibre/martin/pull/2158))
- use Path instead of PathBuf for strs ([#2130](https://github.com/maplibre/martin/pull/2130))
- *(core)* move sprites to `martin_core` ([#2105](https://github.com/maplibre/martin/pull/2105))
- *(core)* move styles to `martin_core` ([#2106](https://github.com/maplibre/martin/pull/2106))
- *(core)* migrate the configuration to be core capable ([#2104](https://github.com/maplibre/martin/pull/2104))
- *(core)* Move fonts to `martin_core` ([#2050](https://github.com/maplibre/martin/pull/2050))
- *(core)* move intial part of the tile catalog to be `martin_core` (3/n) ([#2049](https://github.com/maplibre/martin/pull/2049))
- *(core)* migrate environment tracking and testing (2/n) ([#2048](https://github.com/maplibre/martin/pull/2048))
- *(core)* moved the config utils to `martin-core` (1/n) ([#1944](https://github.com/maplibre/martin/pull/1944))
