# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.7.0](https://github.com/maplibre/martin/compare/martin-v1.6.0...martin-v1.7.0) - 2026-04-23

### `martin_tile_cache_requests_total` and `martin_cache_requests_total` metrics

We have added the following metrics, allowing for knowing what your cache hit rate.
These are two metrics because for tiles we include the zoom while for fonts/sprites this does not make sense.

```raw
# HELP martin_cache_requests_total Martin cache lookups, labeled by cache type and hit/miss result
# TYPE martin_cache_requests_total counter
martin_cache_requests_total{cache="font",result="miss"} NUMBER
martin_cache_requests_total{cache="sprite",result="miss"} NUMBER
# HELP martin_tile_cache_requests_total Martin tile-coordinate cache lookups, labeled by cache type, hit/miss result, and zoom
# TYPE martin_tile_cache_requests_total counter
martin_tile_cache_requests_total{cache="tile",result="hit",zoom="0"} NUMBER
martin_tile_cache_requests_total{cache="tile",result="miss",zoom="0"} NUMBER
```

> [!TIP]
> If you have concrete needs for what metrics you would like to see, please open an issue.
> The set of metrics we offer is quite early in its development livecycle.

### Stabilised Server-side raster tile rendering backend

We have stabilised our rendering backend, which means that you can now render images using MapLibre Native.
We have some work planned to improve performance by prefetching and better paralelism, or to add capabilites like overlaying lines/text/shapes.. via query params.
If you have needs/interests towards this area, we would also invite you to open a discussion/issue on the API that you would like to see.
If you need configurability, we would also like to know what kind of configurability you need.

To enable this feature, you need to add the following to your configuration file:

```yaml
styles:
    rendering: true
```

### Added

- *(ui)* Add Tile URLs TileJSON and XYZ Tiles URLs to the inspect UI ([#2731](https://github.com/maplibre/martin/pull/2731))
- *(mbtiles)* add --strict flag to use STRICT SQLite tables ([#2712](https://github.com/maplibre/martin/pull/2712))

### Fixed

- Keep /health available with `--route-prefix foo` instead of just moving it to /foo/health to enable docker healthchecks ([#2723](https://github.com/maplibre/martin/pull/2723))

### Other

- Some refactorings to increase CI reliability ([#2724](https://github.com/maplibre/martin/pull/2724), [#2715](https://github.com/maplibre/martin/pull/2715), [#2725](https://github.com/maplibre/martin/pull/2725))
- *(deps)* autoupdate pre-commit ([#2720](https://github.com/maplibre/martin/pull/2720))

## [1.6.0](https://github.com/maplibre/martin/compare/martin-v1.5.0...martin-v1.6.0) - 2026-04-18

### Smarter, more configurable caching

The tile cache received several improvements in this release:

- **Configurable cache expiry**
  The in-memory tile cache Time To Live (TTL -> f.ex. `cache.expiry: 1h`) and Time To Idle (TTI ->  f.ex. `cache.idle_timeout: 20m`) was previously hardcoded to "∞" (aka never expiring).
  You can now configure how long cached tiles stay in memory, allowing better trade-offs between freshness and performance for your specific workload.

  ```yaml
  cache:
    # Maximum lifetime for all cache entries (time-to-live from creation).
    # Entries are evicted after this duration regardless of access.
    # Supports human-readable formats: "1h", "30m", "1d", "3600s".
    # default: null (no expiry, entries only evicted by size pressure)
    expiry: null

    # Maximum idle time for all cache entries (time-to-idle since last access).
    # Entries are evicted if not accessed within this duration.
    # default: null (no idle timeout)
    idle_timeout: null
  ```

  Done in [#2691](https://github.com/maplibre/martin/pull/2691).
- **Per-source cache zoom levels**
  New `cache.minzoom` and `cache.maxzoom` options (both globally and per-source) let you skip caching at zoom levels that don't benefit from it.
  For example, you can avoid filling the cache with rarely-reused high-zoom tiles or low-detail overviews.

  ```yaml
  cache:
    # Default minimum zoom level (inclusive) for tile caching.
    # Tiles further zoomed out than this will bypass the cache entirely.
    # Can be overridden per-source (e.g. cache.minzoom on a type of source or an individual source).
    # default: null (no lower bound, all zoom levels cached)
    minzoom: null

    # Default maximum zoom level (inclusive) for tile caching.
    # Tiles further zoomed in than this will bypass the cache entirely.
    # Can be overridden per-source.
    # default: null (no upper bound, all zoom levels cached)
    maxzoom: null
  ```

  Done in [#2673](https://github.com/maplibre/martin/pull/2673) by [@carderne](https://github.com/carderne).
- **Cache deduplication under concurrency**
  Cache insertions now use moka's entry API, so concurrent requests for the same tile only compute it once instead of redundantly.
  This is a meaningful performance win under thundering-herd scenarios.
  Done in [#2688](https://github.com/maplibre/martin/pull/2688).
- **Accept header in cache key** -- The sanitised `Accept` HTTP header is now part of the cache key, preventing a cached response encoded for one client from being incorrectly served to another.
  This **previously did not have any effect and was also not incorrect**, but in the next release we will add MLT encoding support (which we worked hard for).

  This also has the side-effect that if your client now says that you only `Accept` a certain format, we now correctly abort requests early.
  Done in [#2703](https://github.com/maplibre/martin/pull/2703).

### Broader MBTiles compatibility

- **Planetiler `normalized` schema alias** -- Martin now recognizes Planetiler's `normalized` and `normalized-with-view` schema names as aliases for its own `norm` schema type, so MBTiles files produced by Planetiler no longer trigger schema-detection warnings. Done in [#2681](https://github.com/maplibre/martin/pull/2681).
- **Compression type stored in metadata** -- When writing tiles to MBTiles (e.g. via martin-cp), the compression method (gzip, brotli, etc.) is now recorded in the metadata table. Previously this information was lost, forcing consumers to guess. Done in [#2618](https://github.com/maplibre/martin/pull/2618).
- **Transcoder API for library consumers** -- The `mbtiles` crate now exposes a public API for converting between MBTiles storage schemas (flat, normalized, deduplicated) programmatically. Done in [#2682](https://github.com/maplibre/martin/pull/2682).

### `--on-invalid` CLI argument

The `on_invalid` setting (which controls whether Martin warns or aborts when it encounters an invalid source at startup) was previously config-file-only. It is now available as `--on-invalid <warn|abort>` on the command line, which is especially handy in CI/CD and container environments.

Done in [#2668](https://github.com/maplibre/martin/pull/2668) by [@Auspicus](https://github.com/Auspicus).

### Other

- Introduced `TileSourceManager` and `ReloadAdvisory` as groundwork for future live-reload of tile sources ([#2661](https://github.com/maplibre/martin/pull/2661)) by [@Auspicus](https://github.com/Auspicus)
- Added hotpath-based profiling integration ([#2663](https://github.com/maplibre/martin/pull/2663))
- Enabled React Compiler for martin-ui and demo frontend ([#2686](https://github.com/maplibre/martin/pull/2686))
- Enabled `clippy::unwrap_used` workspace lint ([#2670](https://github.com/maplibre/martin/pull/2670))
- Ensured unit tests run on macOS ([#2648](https://github.com/maplibre/martin/pull/2648)) by [@Weixing-Zhang](https://github.com/Weixing-Zhang)
- Various dependency bumps ([#2702](https://github.com/maplibre/martin/pull/2702), [#2684](https://github.com/maplibre/martin/pull/2684), [#2624](https://github.com/maplibre/martin/pull/2624), [#2662](https://github.com/maplibre/martin/pull/2662), [#2657](https://github.com/maplibre/martin/pull/2657))

## [1.5.0](https://github.com/maplibre/martin/compare/martin-v1.4.0...martin-v1.5.0) - 2026-04-02

### Fixed

- *(postgres)* startup crash when ST_Extent returns LineString instead of Polygon ([#2600](https://github.com/maplibre/martin/pull/2600))

### Other

- typos ([#2651](https://github.com/maplibre/martin/pull/2651))
- migrate to workspaced justfiles using `mod` for demo and martin-ui ([#2623](https://github.com/maplibre/martin/pull/2623))
- *(deps)* Bump the all-npm-security-updates group across 2 directories with 1 update ([#2647](https://github.com/maplibre/martin/pull/2647))
- Enable `clippy::use_self` at workspace level and resolve all violations ([#2645](https://github.com/maplibre/martin/pull/2645))
- *(deps-dev)* Bump flatted from 3.3.3 to 3.4.2 in /martin/martin-ui in the all-npm-security-updates group across 1 directory ([#2640](https://github.com/maplibre/martin/pull/2640))
- *(perf)* don't test pg twice ([#2619](https://github.com/maplibre/martin/pull/2619))

## [1.4.0](https://github.com/maplibre/martin/compare/martin-v1.3.1...martin-v1.4.0) - 2026-03-14

### ZSTD support

If your browser prefers this, we will now start sending ZSTD (or deflate) compressed tiles your way.
Done in [#2597](https://github.com/maplibre/martin/pull/2597) by [@nuts-rice](https://github.com/nuts-rice)

### A new documentation site

We migrated our documentation to zenzical, a more modern documentation platform.
Just have a look for yourself, does it not look pretty? -> https://maplibre.org/martin
Done in [#2576](https://github.com/maplibre/martin/pull/2576) by [@
manbhav234](https://github.com/
manbhav234)

### Added

- *(martin-cp)* now has a prettier, indicativ based progress bar ([#2495](https://github.com/maplibre/martin/pull/2495))
- Add retry mechanism on locked/busy mbtiles files was added ([#2572](https://github.com/maplibre/martin/pull/2572))

### Fixed

- *(ui)* render MLT tiles correctly in Tile Inspector ([#2601](https://github.com/maplibre/martin/pull/2601))
- redirect ignoring `--route-prefix` for .pbf tile requests ([#2599](https://github.com/maplibre/martin/pull/2599))
- restrict zooming and panning on data inspector ([#2574](https://github.com/maplibre/martin/pull/2574))
- Accept any INT-containing type in MBTiles validation to be an integer ([#2560](https://github.com/maplibre/martin/pull/2560))

### Other

- rename the `webp.sql` fixture to `webp-no-primary.sql` ([#2564](https://github.com/maplibre/martin/pull/2564))
- more cfg magic instead of #[allow(unused_variables)] ([#2563](https://github.com/maplibre/martin/pull/2563))
- More restrictive expects ([#2562](https://github.com/maplibre/martin/pull/2562))
- feature-gate PostgreSQL tests to remove external dependencies from `cargo test` ([#2558](https://github.com/maplibre/martin/pull/2558))
- Bump some dependencies ([#2608](https://github.com/maplibre/martin/pull/2608), [#2602](https://github.com/maplibre/martin/pull/2602), [#2592](https://github.com/maplibre/martin/pull/2592), [#2577](https://github.com/maplibre/martin/pull/2577), [#2575](https://github.com/maplibre/martin/pull/2575), [#2567](https://github.com/maplibre/martin/pull/2567))

## [1.3.1](https://github.com/maplibre/martin/compare/martin-v1.3.0...martin-v1.3.1) - 2026-02-11

### Added

- *(srv)* Add HTTP 301 redirects for common URL mistakes ([#2528](https://github.com/maplibre/martin/pull/2528))
- *(unstable-cog)* Change tile path semantics for COG sources to match other sources, expose COG bounds, center and tileSize in TileJSON ([#2510](https://github.com/maplibre/martin/pull/2510))

### Fixed

- Fixed the `path-prefix` feature when it comes to frontend assets and log output ([#2549](https://github.com/maplibre/martin/pull/2549), [#2541](https://github.com/maplibre/martin/pull/2541))

### Other

- Add test coverage for header handling in tilejson requests ([#2529](https://github.com/maplibre/martin/pull/2529))
- *(martin-core)* [**breaking**] remove the configuration from the martin-core crate ([#2521](https://github.com/maplibre/martin/pull/2521))
- restrict `unused_trait_names` for trait imports ([#2542](https://github.com/maplibre/martin/pull/2542))
- *(deps)* Bump various dependencies ([#2553](https://github.com/maplibre/martin/pull/2553), [#2545](https://github.com/maplibre/martin/pull/2545), [#2533](https://github.com/maplibre/martin/pull/2533))

## [1.3.0](https://github.com/maplibre/martin/compare/martin-v1.2.0...martin-v1.3.0) - 2026-01-27

### More flexible log formatting

We migrated our `log` library to `tracing`.
This gives us a few internal improvements, but also allows us to introduce a new `RUST_LOG_FORMAT` environment variable.
The available values are: `json`, `full`, `compact` (default), `bare` or `pretty`.

Done in [#2494](https://github.com/maplibre/martin/pull/2494), [#2508](https://github.com/maplibre/martin/pull/2508), [#2500](https://github.com/maplibre/martin/pull/2500) by @CommanderStorm

### Glyph ranges beyond `0xFFFF`

If you are using fonts which span beyond the `0xFFFF` range, this release improves how Martin loads and renders those glyphs so they are handled correctly.

Here is a short explanation of why this might matter to you based on <https://en.wikipedia.org/wiki/Unicode_block>.

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


- *(deps)* `cargo-shear` our dependencies for improved compile times ([#2497](https://github.com/maplibre/martin/pull/2497))
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
- *(deps)* miscellaneous dependency bumps ([#2403](https://github.com/maplibre/martin/pull/2403), [#2404](https://github.com/maplibre/martin/pull/2404), [#2373](https://github.com/maplibre/martin/pull/2373), [#2375](https://github.com/maplibre/martin/pull/2375), [#2374](https://github.com/maplibre/martin/pull/2374))

## [1.0.0](https://github.com/maplibre/martin/compare/martin-v0.20.2...martin-v1.0.0) - 2025-11-10

🎉🎉🎉 **After 8 years in developmen, we are excited to release v1.0.0 of martin.** 🎉🎉🎉
Functionally, it is the same as `v0.20.2`, just with our releases further automated.
There are no breaking changes between `v0.20.X` and `v1.X.X`

### Fixed

- broken url to github release in web-ui ([#2354](https://github.com/maplibre/martin/pull/2354))

## [0.20.2](https://github.com/maplibre/martin/compare/martin-v0.20.1...martin-v0.20.2) - 2025-11-07

In 0.20.1 we clamed to have fixed the bug regarding how our release script determines versions for docker containers.
This was incorrect and is fixed now with a more manual approach instead of relying on `docker/metadata-action`.
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

For further details on the now available options, please [see our documentation](https://maplibre.org/martin/sources-files.html).

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
- *(config)* refactor the lifecycle hooks to be cleaner and better documented ([#2282](https://github.com/maplibre/martin/pull/2282))
- *(lints)* apply tighter clippy lints like `clippy::panic_in_result_fn`, `clippy::todo` or similar [#2284](https://github.com/maplibre/martin/pull/2284 [#2283](https://github.com/maplibre/martin/pull/2283), [#2288](https://github.com/maplibre/martin/pull/2288), [#2287](https://github.com/maplibre/martin/pull/2287)
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
- improve doc comment for `IdResolver::resolve` to remove ambiguity if we should log for reserved names ([#2065](https://github.com/maplibre/martin/pull/2065))

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
- *dependencies* And a bunch of dependency updates ([#2218](https://github.com/maplibre/martin/pull/2218), [#2189](https://github.com/maplibre/martin/pull/2189)), [#2161](https://github.com/maplibre/martin/pull/2161), [#2143](https://github.com/maplibre/martin/pull/2143), [#2147](https://github.com/maplibre/martin/pull/2147), [#2139](https://github.com/maplibre/martin/pull/2139), [#2128](https://github.com/maplibre/martin/pull/2128), [#2135](https://github.com/maplibre/martin/pull/2135), [#2125](https://github.com/maplibre/martin/pull/2125), [#2126](https://github.com/maplibre/martin/pull/2126), [#2120](https://github.com/maplibre/martin/pull/2120), [#2119](https://github.com/maplibre/martin/pull/2119), [#2111](https://github.com/maplibre/martin/pull/2111), [#2082](https://github.com/maplibre/martin/pull/2082), [#2093](https://github.com/maplibre/martin/pull/2093), [#2084](https://github.com/maplibre/martin/pull/2084), [#2081](https://github.com/maplibre/martin/pull/2081), [#2069](https://github.com/maplibre/martin/pull/2069), [#2070](https://github.com/maplibre/martin/pull/2070), [#2013](https://github.com/maplibre/martin/pull/2013), [#2106](https://github.com/maplibre/martin/pull/2106), [#2104](https://github.com/maplibre/martin/pull/2104), [#2050](https://github.com/maplibre/martin/pull/2050), [#2049](https://github.com/maplibre/martin/pull/2049), [#2100](https://github.com/maplibre/martin/pull/2100), [#2232](https://github.com/maplibre/martin/pull/2232))
