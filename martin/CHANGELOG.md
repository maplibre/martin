# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.3.0](https://github.com/maplibre/martin/compare/martin-v1.2.0...martin-v1.3.0) - 2026-01-27

### More flexible log formatting

We migrated our `log` library to `tracing`.
This gives us a few internal improvements, but also allows us to introduce a new `RUST_LOG_FORMAT` environment variable.
The avaliable values are: `json`, `full`, `compact` (default), `bare` or `pretty`.

Done in [#2494](https://github.com/maplibre/martin/pull/2494), [#2508](https://github.com/maplibre/martin/pull/2508), [#2500](https://github.com/maplibre/martin/pull/2500) by @CommanderStorm

### Glyph ranges beyond `0xFFFF`

If you are using fonts which span beyond the `0xFFFF`

Here is a short explaination of why this might matter to you based on <https://en.wikipedia.org/wiki/Unicode_block>.

- `U+0000` - `U+FFFF` is Basic Multilingual Plane, which covers characters for almost all modern languages
- `U+10000` - `U+3347F` covers minor characters such as historic scripts and emojis
- `U+E0000` - `U+E01EF` is for tags and variation selectors
- `U+F0000` - `U+10FFFF` is for private use (i.e. can be assigned arbitrary custom characters without worrying about possible conflict with the future standards)

Done in ([#2438](https://github.com/maplibre/martin/pull/2438)) by @yutannihilation

As a related performance optimisation, we also removed `FontSources.masks` as it was consuming large amounts of memory and some startup time, even when no font sources were set ([#2519](https://github.com/maplibre/martin/pull/2519)) by @Auspicus

### Simpler native subpath support

We added the `route_prefix` configuration and `--route-prefix` cli arguments.
This allows you to configure the subpath martin is serving from without the need for your reverse proxy to strip these subpaths before getting to us.

Done in ([#2523](https://github.com/maplibre/martin/pull/2523))

### MLT decoding support

Martin now supports the MapLibre Tiles Specification.
This means that if you want to serve MLT based tiles with this tileserver, you now can.
Read more about what the MapLibre Tile Specification is and why we are "reinventing the wheel on this one" in our [blog post](https://maplibre.org/news/2026-01-23-mlt-release/).

Done in ([#2512](https://github.com/maplibre/martin/pull/2512))

### Added

- improve martin-cp progress output time estimate by displaying in human time instead of seconds ([#2491](https://github.com/maplibre/martin/pull/2491))
- *(pg)* support PostgreSQL materialized views ([#2279](https://github.com/maplibre/martin/pull/2279))
- *(pg)* include ID column info for tables ([#2485](https://github.com/maplibre/martin/pull/2485))

### Fixed

- improve error message if no SVG sprite files are present ([#2516](https://github.com/maplibre/martin/pull/2516))
- *(ui)* Fix clipboard copy for <http://0.0.0.0:3000> and unify implementations and their design ([#2487](https://github.com/maplibre/martin/pull/2487), [#2489](https://github.com/maplibre/martin/pull/2489), [#2483](https://github.com/maplibre/martin/pull/2483), [#2482](https://github.com/maplibre/martin/pull/2482))

### Other


- *(deps)* `cargo-shear` our dependencys for improved compile times ([#2497](https://github.com/maplibre/martin/pull/2497))
- *(mbtiles)* improve a few test cases ([#2478](https://github.com/maplibre/martin/pull/2478), [#2480](https://github.com/maplibre/martin/pull/2480), [#2477](https://github.com/maplibre/martin/pull/2477))

## [1.2.0](https://github.com/maplibre/martin/compare/martin-v1.1.0...martin-v1.2.0) - 2026-01-03

### Optionally fail config loading/resolution for missing sources

We added the `on_invalid: abort` (default) and `on_invalid: warn` settings, which controls what happens when martin encounters an missing/invalid source.

Done in [#2412](https://github.com/maplibre/martin/pull/2412), [#2426](https://github.com/maplibre/martin/pull/2426) by @gabeschine

### Click to copy when clicking on various IDs in the UI

When clicking on various IDs in the UI, a click to copy feature is now available.

<img width="720" height="393" alt="image" src="https://github.com/user-attachments/assets/6d062bb1-18f1-4827-a378-44b141eb11d5" />

Done in [#2427](https://github.com/maplibre/martin/pull/2427) by @todtb

### Fixed

- *(pg)* Instead of reporting on all available tables, we now filter the result to the configured sources when `auto_publish: false` ([#2411](https://github.com/maplibre/martin/pull/2411))
- *(sprites)* Scale SDF buffer and radius by pixel ratio leading to weird artefacts when using retina sdf sprites ([#2458](https://github.com/maplibre/martin/pull/2458))

### Other

- made our dependency management more reproducible/stable ([#2442](https://github.com/maplibre/martin/pull/2442), [#2429](https://github.com/maplibre/martin/pull/2429), [#2415](https://github.com/maplibre/martin/pull/2415))
- various dependency bumps ([#2439](https://github.com/maplibre/martin/pull/2439), [#2435](https://github.com/maplibre/martin/pull/2435), [#2418](https://github.com/maplibre/martin/pull/2418), [#2471](https://github.com/maplibre/martin/pull/2471))
- *(bench)* improve benchmark accuracy by adding black_box for tables/functions ([#2413](https://github.com/maplibre/martin/pull/2413))
- *(pmtiles)* add pmtiles test in `martin-core` ([#2443](https://github.com/maplibre/martin/pull/2443))

## [1.1.0](https://github.com/maplibre/martin/compare/martin-v1.0.0...martin-v1.1.0) - 2025-12-11

### Added

- *(martin-cp)* infer default bounds from configured sources for better performance ([#2385](https://github.com/maplibre/martin/pull/2385))
- *(martin-core)* add an pmtiles example ([#2370](https://github.com/maplibre/martin/pull/2370))

### Fixed

- allow `az://` URL schemes in discovery ([#2408](https://github.com/maplibre/martin/pull/2408))

### Other

- *(config)* move the resolve impl to a different function ([#2397](https://github.com/maplibre/martin/pull/2397))
- *(docs)* fix martin-cp bbox docs ([#2387](https://github.com/maplibre/martin/pull/2387))
- *(deps)* miscelaneous dependency bumps ([#2403](https://github.com/maplibre/martin/pull/2403), [#2404](https://github.com/maplibre/martin/pull/2404), [#2373](https://github.com/maplibre/martin/pull/2373), [#2375](https://github.com/maplibre/martin/pull/2375), [#2374](https://github.com/maplibre/martin/pull/2374))

## [1.0.0](https://github.com/maplibre/martin/compare/martin-v0.20.2...martin-v1.0.0) - 2025-11-10

🎉🎉🎉 **After 8 years in developmen, we are excited to release v1.0.0 of martin.** 🎉🎉🎉
Functionally, it is the same as `v0.20.2`, just with our releses further automated.
There are no breaking changes between `v0.20.X` and `v1.X.X`

### Fixed

- broken url to github release in web-ui ([#2354](https://github.com/maplibre/martin/pull/2354))

## [0.20.2](https://github.com/maplibre/martin/compare/martin-v0.20.1...martin-v0.20.2) - 2025-11-07

In 0.20.1 we clamed to have fixed the bug regarding how our release script determines versions for docker containers.
This was incorrect and is fixed now with a more manual appraoch instead of relying on `docker/metadata-action`.
Done in [#2348](https://github.com/maplibre/martin/pull/2348)

### Other

- Remove unused optional 'tiff' dependency from Cargo.toml ([#2343](https://github.com/maplibre/martin/pull/2343))

## [0.20.1](https://github.com/maplibre/martin/compare/martin-v0.20.0...martin-v0.20.1) - 2025-11-03

## Fixed prefixes in ghcr tags

We fixed a bug where in the 0.20.0 release our ghcr.io tags always had the prefix `:martin-v0.20.0` and were also published under `:martin-core-v0.2.0` and `:mbtiles-v0.14.0`.

Sorry for users affected by this change.
Done in [#2338](https://github.com/maplibre/martin/pull/2338)

## Fix

Fixed a potential crash due to an off-by-one error when zooming in at exactly Zoom 30 (our limit). [#2340](https://github.com/maplibre/martin/pull/2340)

## Maintenance

- *(ci)* add pre commit step to sync the fronted version to the backend ([#2324](https://github.com/maplibre/martin/pull/2324))
- reduce pg discovery bench sizes ([#2321](https://github.com/maplibre/martin/pull/2321))
- various dependency bumps ([#2331](https://github.com/maplibre/martin/pull/2331), [#2333](https://github.com/maplibre/martin/pull/2333), [#2332](https://github.com/maplibre/martin/pull/2332))

## [0.20.0](https://github.com/maplibre/martin/compare/martin-v0.19.3...martin-v0.20.0) - 2025-10-27


> [!NOTE]
> This release can be considered the last beta of the v1.0 release.
> We have locked down key parts of the architecture.
>
> We will republish this release as v1.0 in roghly a week, unless we see any bugs in this release.

A big thank you to everyone who contributed to this release - through code, reviews, testing, and feedback.
Your work and discussions continue to make Martin faster, more reliable, and more welcoming for new users.

We couldn’t have done it without you ❤️

### A better, more configurable cache

In previous versions, the cache was a single monolithic cache.
We have split this up into different parts and you can now specify how much sprites, fonts, pmtiles directories and tiles martin is allowed in the cache.

> [!TIP]
> We also now support caching sprites and fonts - speeding up the rendering of vector maps.

See our [documentation here](https://maplibre.org/martin/config-file.html) for further context.

Done in [#2295](https://github.com/maplibre/martin/pull/2295) [#2304](https://github.com/maplibre/martin/pull/2304) [#2303](https://github.com/maplibre/martin/pull/2303), [#2297](https://github.com/maplibre/martin/pull/2297)

### Pmtiles support for Google Cloud, Azure and much more options

The good news first:
- [greatly expanded](https://maplibre.org/martin/sources-files.html) options for AWS and HTTP backends
- New support for Google Cloud and Azure object storage
- Local files remain unaffected

How did we do this?
We replaced our entire pmtiles backend with the [`object_storage` crate](http://docs.rs/object_storage).

Most of the options are cleanly migratable, but we deprecated the following:

- AWS specific environment variable usages are deprecated.
- `pmtiles.allow_http` being unset is currently defaulting to `true`.
  In v2.0, we will change this to be `false` by default for better security defaults.

The deprecated items will be removed in v2.0 at the earliest.

> [!TIP]
> Each of the deprecations also has its own warning in the log, so you don't have to guess if you are affected 😉

`AWS_PROFILE` presented a challenge and we had to drop this environment variable.
We asked for community feedback on Slack (see [here](https://maplibre.org/community)), and it seems this may not be a necessary feature.
If you depend on `AWS_PROFILE`, we opened the following issue to discuss details:
- https://github.com/maplibre/martin/issues/2286

For further details on the now avaliable options, please [see our documentation](https://maplibre.org/martin/sources-files.html).

Done in [#2251](https://github.com/maplibre/martin/pull/2251)


### unstable style rendering support

We added an experimental option for server-side style rendering, allowing you to convert your configured styles into images on the server side instead of the client.
See our [documentation here](https://maplibre.org/martin/sources-styles.html#server-side-raster-tile-rendering) for further context.

Done in [#2306](https://github.com/maplibre/martin/pull/2306)

### rename `cog` feature to `unstable-cog`

The `cog` feature was renamed to `unstable-cog` and thus removed from the features active by default.
If you compile martin from source with this feature enabled, experimentation is still possible.
This change signals that the feature is still evolving and allows us to iterate more freely as we add the missing functionality.
Currently, our COG support does not support certain projection aspects required for good usability.

Done in [#2285](https://github.com/maplibre/martin/pull/2285)

### Removal of deprecated functionality

We removed the long-deprecated `--watch` CLI option, which previously only displayed a deprecation warning in the log.

Done in [#2294](https://github.com/maplibre/martin/pull/2294)

### Fix

- Make mbtiles dependency properly optional again ([#2292](https://github.com/maplibre/martin/pull/2292))

### Other

- *(core)* enable overriding of the automatic hashing for source traits ([#2293](https://github.com/maplibre/martin/pull/2293))
- *(pg)* Add benchmark for source discovery timing ([#2263](https://github.com/maplibre/martin/pull/2263))
- *(admin)* move functionality into better modules ([#2315](https://github.com/maplibre/martin/pull/2315))
- move config files to new folders ([#2298](https://github.com/maplibre/martin/pull/2298))
- *(core)* add a `_tiles` feature  to simplify our feature configuration ([#2296](https://github.com/maplibre/martin/pull/2296))
- *(config)* refactor the livecycle hooks to be cleaner and better documented ([#2282](https://github.com/maplibre/martin/pull/2282))
- *(lints)* applly tighter clippy lints like `clippy::panic_in_result_fn`, `clippy::todo` or similar [#2284](https://github.com/maplibre/martin/pull/2284 [#2283](https://github.com/maplibre/martin/pull/2283), [#2288](https://github.com/maplibre/martin/pull/2288), [#2287](https://github.com/maplibre/martin/pull/2287)
- *(mbtiles)* Generate mbtiles dynamically from SQL files to increase debuggability, transparency and supply chain trust/security ([#1868](https://github.com/maplibre/martin/pull/1868))
- A number of dependency updates [#2277](https://github.com/maplibre/martin/pull/2277), [#2308](https://github.com/maplibre/martin/pull/2308)


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
