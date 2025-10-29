# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.20.0](https://github.com/maplibre/martin/compare/martin-v0.19.3...martin-v0.20.0) - 2025-10-29

### Added

- unstable style rendering ([#2306](https://github.com/maplibre/martin/pull/2306))
- *(cache)* implement sprite caching ([#2295](https://github.com/maplibre/martin/pull/2295))
- add font caching ([#2304](https://github.com/maplibre/martin/pull/2304))
- *(cache)* [**breaking**] split the cache configuration of tiles and pmtiles directories ([#2303](https://github.com/maplibre/martin/pull/2303))
- *(core)* enable overriding of the automatic hashing for source traits ([#2293](https://github.com/maplibre/martin/pull/2293))
- *(pg)* Add benchmark for source discovery timing ([#2263](https://github.com/maplibre/martin/pull/2263))
- *(pmtiles)* [**breaking**] change pmtiles to base the implementation on `object_storage` instead ([#2251](https://github.com/maplibre/martin/pull/2251))

### Fixed

- *(cog)* [**breaking**] rename `cog` feature to `unstable-cog` ([#2285](https://github.com/maplibre/martin/pull/2285))

### Other

- *(admin)* move functionality into better modules ([#2315](https://github.com/maplibre/martin/pull/2315))
- *(deps-dev)* Bump vite from 7.1.7 to 7.1.11 in /martin/martin-ui in the all-npm-ui-security-updates group across 1 directory ([#2308](https://github.com/maplibre/martin/pull/2308))
- *(lints)* enable `clippy::unimplemented` and `clippy::panic` ([#2287](https://github.com/maplibre/martin/pull/2287))
- *(lints)* audit all allows, add reasons and remove unnessesary ones ([#2288](https://github.com/maplibre/martin/pull/2288))
- move config files to new folders ([#2298](https://github.com/maplibre/martin/pull/2298))
- *(core)* add a `_tiles` feature  to simplify our feature configuration ([#2296](https://github.com/maplibre/martin/pull/2296))
- move `MainCache` to be a `TileCache` ([#2297](https://github.com/maplibre/martin/pull/2297))
- *(config)* [**breaking**] remove deprecated `--watch` from the CLI options and `MbtilesPool::new` ([#2294](https://github.com/maplibre/martin/pull/2294))
- Make mbtiles dependency properly optional again ([#2292](https://github.com/maplibre/martin/pull/2292))
- *(config)* refactor the livecycle hooks to be cleaner and better documented ([#2282](https://github.com/maplibre/martin/pull/2282))
- *(lints)* migrate a few of our expects to unwraps ([#2284](https://github.com/maplibre/martin/pull/2284))
- *(lints)* applly `clippy::panic_in_result_fn` and `clippy::todo` as warnings ([#2283](https://github.com/maplibre/martin/pull/2283))
- *(mbtiles)* Generate mbtiles dynamically from SQL files to increase debuggability and transparency ([#1868](https://github.com/maplibre/martin/pull/1868))
- *(deps-dev)* Bump the all-npm-ui-version-updates group in /martin/martin-ui with 2 updates ([#2277](https://github.com/maplibre/martin/pull/2277))
- release ([#2265](https://github.com/maplibre/martin/pull/2265))

## [0.19.3](https://github.com/maplibre/martin/compare/martin-v0.19.2...martin-v0.19.3) - 2025-10-01

### Added

- add `tilejson_url_version_param` configuration which allows embedding the version of tile sources (specifically pmtiles) in tilejson tiles URL, resulting in better cache hit rates ([#2198](https://github.com/maplibre/martin/pull/2198))

### Other

- fix docs.rs not build failing due to misconfiguration of the `cfg(feature="webui")` ([#2273](https://github.com/maplibre/martin/pull/2273))
- release ([#2265](https://github.com/maplibre/martin/pull/2265))

## [0.19.2](https://github.com/maplibre/martin/compare/martin-v0.19.1...martin-v0.19.2) - 2025-09-28

### Other

- The previous `0.19.1` release had another bug which prevented proper releasing which is now fixed ([#2262](https://github.com/maplibre/martin/pull/2262))
- update Cargo.lock dependencies

## [0.19.1](https://github.com/maplibre/martin/compare/martin-v0.19.0...martin-v0.19.1) - 2025-09-28

### Fixed

- *(release)* Our release new process for `0.19.0` did not properly attach binaries and build docker files due to permission issues ([#2253](https://github.com/maplibre/martin/pull/2253), [#2260](https://github.com/maplibre/martin/pull/2260))

## [0.19.0](https://github.com/maplibre/martin/compare/martin-v0.18.1...martin-v0.19.0) - 2025-09-26

### Breaking Changes

- we migrated our internal codebase to be split into `martin-core` and `martin`. While this does have **NO** have an **public facing impact** for **API, Configuration and behaviour**, this ensures that we can release v1.0 without breaking the SemVer promise. If you previously used **`martin` as a crates.io library, please use `martin-core` instead**. ([#2227](https://github.com/maplibre/martin/pull/2227),[#2215](https://github.com/maplibre/martin/pull/2215),[#2217](https://github.com/maplibre/martin/pull/2217),[#2213](https://github.com/maplibre/martin/pull/2213),[#2216](https://github.com/maplibre/martin/pull/2216),[#2192](https://github.com/maplibre/martin/pull/2192),[#2194](https://github.com/maplibre/martin/pull/2194),[#2191](https://github.com/maplibre/martin/pull/2191),[#2185](https://github.com/maplibre/martin/pull/2185),[#2182](https://github.com/maplibre/martin/pull/2182),[#2181](https://github.com/maplibre/martin/pull/2181),[#2184](https://github.com/maplibre/martin/pull/2184),[#2179](https://github.com/maplibre/martin/pull/2179),[#2176](https://github.com/maplibre/martin/pull/2176),[#2178](https://github.com/maplibre/martin/pull/2178),[#2177](https://github.com/maplibre/martin/pull/2177),[#2172](https://github.com/maplibre/martin/pull/2172),[#2171](https://github.com/maplibre/martin/pull/2171),[#2167](https://github.com/maplibre/martin/pull/2167),[#2158](https://github.com/maplibre/martin/pull/2158),[#2157](https://github.com/maplibre/martin/pull/2157),[#2160](https://github.com/maplibre/martin/pull/2160),[#2156](https://github.com/maplibre/martin/pull/2156),[#2105](https://github.com/maplibre/martin/pull/2105),[#2048](https://github.com/maplibre/martin/pull/2048),[#1944](https://github.com/maplibre/martin/pull/1944), [#2159](https://github.com/maplibre/martin/pull/2159))
- *(martin-cp)* The `--cache-size` option has been removed from martin-cp. For most usecases, this is not what you want. ([#2026](https://github.com/maplibre/martin/pull/2026))

### Added

- We updated Martins' Logo ([#1959](https://github.com/maplibre/martin/pull/1959))
- *(config)* Implement unrecognised value in config file warning ([#2151](https://github.com/maplibre/martin/pull/2151), [#2152](https://github.com/maplibre/martin/pull/2152), [#2236](https://github.com/maplibre/martin/pull/2236), [#1967](https://github.com/maplibre/martin/pull/1967))
- *(martin-cp)* add a warning if `--concurrency 1` and an error if `--concurrency 0` ([#2027](https://github.com/maplibre/martin/pull/2027))

### Fixed

- *(docs)* publish docs to docs.rs again ([#2239](https://github.com/maplibre/martin/pull/2239))
- *(ui)* inspect button not working for raster sources ([#2155](https://github.com/maplibre/martin/pull/2155))
- *(pg)* fixed if one has a table and an identically named view in another schemas, tile serving did not work ([#2149](https://github.com/maplibre/martin/pull/2149), [#2112](https://github.com/maplibre/martin/pull/2112))

### Documentation

- reworded the error messages for `InternalError`, `FontError` ([#2226](https://github.com/maplibre/martin/pull/2226))
- add doc comments for pmtiles and mbtiles ([#2164](https://github.com/maplibre/martin/pull/2164))
- document public parts of the  `cog`-module ([#2166](https://github.com/maplibre/martin/pull/2166))
- document the exposed parts of the `pg` module ([#2165](https://github.com/maplibre/martin/pull/2165))
- improve `IdResolver::resolve` warnings ([#2066](https://github.com/maplibre/martin/pull/2066))
- improve doc comment for `IdResolver::resolve` to remove ambiguitiy if we should log for reserved names ([#2065](https://github.com/maplibre/martin/pull/2065))

### Other

- We have automated our release pipeline and are now releasing via `relese-plz` ([#2242](https://github.com/maplibre/martin/pull/2242))
- fix various clippy or related code style issues ([#1904](https://github.com/maplibre/martin/pull/1904), [#1903](https://github.com/maplibre/martin/pull/1903), [#2092](https://github.com/maplibre/martin/pull/2092), [#2052](https://github.com/maplibre/martin/pull/2052), [#2193](https://github.com/maplibre/martin/pull/2193), [#2130](https://github.com/maplibre/martin/pull/2130), [#2233](https://github.com/maplibre/martin/pull/2233))
- Add comprehensive GitHub Copilot instructions for Martin development workflow ([#2210](https://github.com/maplibre/martin/pull/2210))
- *(bench)* add an benchmark that tests the impact of the error variant ([#2168](https://github.com/maplibre/martin/pull/2168))
- *(cog)* use binary snapshots for testing ([#2129](https://github.com/maplibre/martin/pull/2129))
- *(ui)*make sure that the fronted forces esm instead of cjs ([#2038](https://github.com/maplibre/martin/pull/2038))
- *(ui)* migration `jest` to `vitest` ([#2040](https://github.com/maplibre/martin/pull/2040))
- *(ui)* move from eslint to biomejs for formatting/linting ([#1909](https://github.com/maplibre/martin/pull/1909))
- *(ci)* add cargo-sort to consistently sort our `Cargo.toml` ([#2020](https://github.com/maplibre/martin/pull/2020))
- *(ci)* Split tests and lints in CI ([#2225](https://github.com/maplibre/martin/pull/2225))
- *dependencys* And a bunch of dependency updates ([#2218](https://github.com/maplibre/martin/pull/2218), [#2189](https://github.com/maplibre/martin/pull/2189)), [#2161](https://github.com/maplibre/martin/pull/2161), [#2143](https://github.com/maplibre/martin/pull/2143), [#2147](https://github.com/maplibre/martin/pull/2147), [#2139](https://github.com/maplibre/martin/pull/2139), [#2128](https://github.com/maplibre/martin/pull/2128), [#2135](https://github.com/maplibre/martin/pull/2135), [#2125](https://github.com/maplibre/martin/pull/2125), [#2126](https://github.com/maplibre/martin/pull/2126), [#2120](https://github.com/maplibre/martin/pull/2120), [#2119](https://github.com/maplibre/martin/pull/2119), [#2111](https://github.com/maplibre/martin/pull/2111), [#2082](https://github.com/maplibre/martin/pull/2082), [#2093](https://github.com/maplibre/martin/pull/2093), [#2084](https://github.com/maplibre/martin/pull/2084), [#2081](https://github.com/maplibre/martin/pull/2081), [#2069](https://github.com/maplibre/martin/pull/2069), [#2070](https://github.com/maplibre/martin/pull/2070), [#2013](https://github.com/maplibre/martin/pull/2013), [#2106](https://github.com/maplibre/martin/pull/2106), [#2104](https://github.com/maplibre/martin/pull/2104), [#2050](https://github.com/maplibre/martin/pull/2050), [#2049](https://github.com/maplibre/martin/pull/2049), [#2100](https://github.com/maplibre/martin/pull/2100), [#2232](https://github.com/maplibre/martin/pull/2232))
