# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
