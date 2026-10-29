# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/maplibre/martin/compare/map-tile-toolkit-v0.3.4...map-tile-toolkit-v0.4.0) - 2026-10-29

### Fixed

- *(toolkit)* `Quantizer` rounds a vertex just off a half to the nearer unit ([#3778](https://github.com/maplibre/martin/pull/3778))
- *(tilegen)* bulk mode keeps what lies on a tile's buffer edge, as tile mode does ([#3767](https://github.com/maplibre/martin/pull/3767))
- *(tilegen)* bulk mode rounds halves to even, as tile mode does ([#3766](https://github.com/maplibre/martin/pull/3766))
- *(tilegen)* MultiPolygon parts that round onto each other merge instead of overlapping ([#3752](https://github.com/maplibre/martin/pull/3752))

### Other

- *(toolkit)* check small polygons without the grid in Quantizer ([#3832](https://github.com/maplibre/martin/pull/3832))
- *(toolkit)* round halves to even with one remainder match ([#3769](https://github.com/maplibre/martin/pull/3769))
- Use callgrind mode for hotpath benchmark ([#3760](https://github.com/maplibre/martin/pull/3760))
- *(toolkit)* share Quantizer's edge tests and ring helpers ([#3723](https://github.com/maplibre/martin/pull/3723))

## [0.3.4](https://github.com/maplibre/martin/compare/map-tile-toolkit-v0.3.3...map-tile-toolkit-v0.3.4) - 2026-10-22

### Added

- move map-tile-toolkit into the workspace ([#3648](https://github.com/maplibre/martin/pull/3648))

### Other

- import local items via `crate::` paths ([#3667](https://github.com/maplibre/martin/pull/3667))
- format Rust code at 120 columns ([#3673](https://github.com/maplibre/martin/pull/3673))

## [0.3.3](https://github.com/nyurik/map-tile-toolkit/compare/v0.3.2...v0.3.3) - 2026-10-18

### Added

- valid polygon pieces, and a faster path for small polygons ([#40](https://github.com/nyurik/map-tile-toolkit/pull/40))

### Other

- one piece buffer owns the synthetic-edge marks ([#42](https://github.com/nyurik/map-tile-toolkit/pull/42))
- slice a polygon within a window of tiles ([#41](https://github.com/nyurik/map-tile-toolkit/pull/41))

## [0.3.2](https://github.com/nyurik/map-tile-toolkit/compare/v0.3.1...v0.3.2) - 2026-10-15

### Other

- more doc cleanup
- add images
- add images

## [0.3.1](https://github.com/nyurik/map-tile-toolkit/compare/v0.3.0...v0.3.1) - 2026-10-15

### Added

- unclipped slicing mode ([#37](https://github.com/nyurik/map-tile-toolkit/pull/37))

### Other

- *(deps)* bump taiki-e/install-action from 2.87.23 to 2.87.26 in the all-actions-version-updates group ([#36](https://github.com/nyurik/map-tile-toolkit/pull/36))

## [0.3.0](https://github.com/nyurik/map-tile-toolkit/compare/v0.2.0...v0.3.0) - 2026-10-14

### Added

- [**breaking**] lazy one-feature Slicer replaces the accumulating slicers ([#34](https://github.com/nyurik/map-tile-toolkit/pull/34))

### Other

- *(deps)* update hotpath requirement from 0.27.0 to 0.28.4 in the all-cargo-version-updates group ([#32](https://github.com/nyurik/map-tile-toolkit/pull/32))

## [0.2.0](https://github.com/nyurik/map-tile-toolkit/compare/v0.1.0...v0.2.0) - 2026-10-10

### Added

- PolygonSlicerAll, fill runs, and a shared polygon view ([#23](https://github.com/nyurik/map-tile-toolkit/pull/23))

### Fixed

- restore the coverage recipe, with a full clean

### Other

- route segments by an exact tile walk; lift the i16 span and u16 vertex limits ([#22](https://github.com/nyurik/map-tile-toolkit/pull/22))
- improve coverage ([#31](https://github.com/nyurik/map-tile-toolkit/pull/31))
- fast paths for small polygons, crossings indexed during routing ([#29](https://github.com/nyurik/map-tile-toolkit/pull/29))
- drop the baseline benches duplicated by merging #28 after #27 ([#30](https://github.com/nyurik/map-tile-toolkit/pull/30))
- *(deps)* bump taiki-e/install-action from 2.87.4 to 2.87.23 in the all-actions-version-updates group across 1 directory ([#25](https://github.com/nyurik/map-tile-toolkit/pull/25))
- geo and stripe baselines, a buildings scenario ([#28](https://github.com/nyurik/map-tile-toolkit/pull/28))
- one-pass polygon clipping and counting sorts, with geo and stripe baseline benches ([#27](https://github.com/nyurik/map-tile-toolkit/pull/27))
- move ci-bench into benches/ci-bench.sh, counting both sides from sibling worktrees
- instruction-count tables as a sticky PR comment, compared with the base commit
- disable binstall compile in CI
- update release-plz
- sort GeoJSON snapshot features by role ([#24](https://github.com/nyurik/map-tile-toolkit/pull/24))
- *(deps)* update hotpath requirement from 0.26.1 to 0.27.0 in the all-cargo-version-updates group ([#20](https://github.com/nyurik/map-tile-toolkit/pull/20))
- Polygon support ([#10](https://github.com/nyurik/map-tile-toolkit/pull/10))
- *(deps)* bump the all-actions-version-updates group across 1 directory with 2 updates ([#18](https://github.com/nyurik/map-tile-toolkit/pull/18))
- *(deps)* bump the all-cargo-version-updates group across 1 directory with 2 updates ([#19](https://github.com/nyurik/map-tile-toolkit/pull/19))
- *(deps)* bump taiki-e/install-action from 2.85.0 to 2.85.13 in the all-actions-version-updates group across 1 directory ([#14](https://github.com/nyurik/map-tile-toolkit/pull/14))
- *(deps)* update hotpath requirement from 0.22.0 to 0.23.1 in the all-cargo-version-updates group across 1 directory ([#15](https://github.com/nyurik/map-tile-toolkit/pull/15))
- [pre-commit.ci] pre-commit autoupdate ([#13](https://github.com/nyurik/map-tile-toolkit/pull/13))
- switch to Gungraun benchmarks ([#12](https://github.com/nyurik/map-tile-toolkit/pull/12))
- *(deps)* bump taiki-e/install-action from 2.83.4 to 2.85.0 in the all-actions-version-updates group ([#11](https://github.com/nyurik/map-tile-toolkit/pull/11))
- move tests to polyline subdir ([#9](https://github.com/nyurik/map-tile-toolkit/pull/9))
- more tests
- cleanup precommit and changelog
- format geojson tests ([#8](https://github.com/nyurik/map-tile-toolkit/pull/8))

## [0.1.0](https://github.com/nyurik/map-tile-toolkit/tree/v0.1.0) - 2026-07-27
- initial release
