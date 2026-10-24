# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/maplibre/martin/compare/martin-tilegen-v0.1.1...martin-tilegen-v0.1.2) - 2026-10-24

### Added

- *(martin)* `martin test generate` and `run` over real rows ([#3686](https://github.com/maplibre/martin/pull/3686))
- *(martin)* `martin test` paths for lines crossing a tile edge or shorter than a tile unit ([#3697](https://github.com/maplibre/martin/pull/3697))
- *(martin)* a `martin test` path per value the source's filter allows ([#3696](https://github.com/maplibre/martin/pull/3696))
- *(tilegen)* list the paths a feature can take through a layer ([#3685](https://github.com/maplibre/martin/pull/3685))

### Fixed

- *(tilegen)* `martin test` has no paths that `NOT NULL`, enum labels, a `CHECK` or the key rule out ([#3749](https://github.com/maplibre/martin/pull/3749))
- *(tilegen)* MultiPolygon parts that round onto each other merge instead of overlapping ([#3752](https://github.com/maplibre/martin/pull/3752))
- *(tilegen)* stop on geometries a tile cannot hold, naming the feature ([#3756](https://github.com/maplibre/martin/pull/3756))
- *(tilegen)* keep a multipoint whose parts include an EMPTY point ([#3751](https://github.com/maplibre/martin/pull/3751))
- *(generate)* keep unclipped features whole in every tile any part reaches ([#3731](https://github.com/maplibre/martin/pull/3731))
- *(generate)* drop vertices rounded onto a straight run ([#3716](https://github.com/maplibre/martin/pull/3716))

### Other

- *(generate)* run the pushdown and engine-serve PG tests, cover untested bulk paths ([#3739](https://github.com/maplibre/martin/pull/3739))
- *(toolkit)* share Quantizer's edge tests and ring helpers ([#3723](https://github.com/maplibre/martin/pull/3723))

## [0.1.1](https://github.com/maplibre/martin/compare/martin-tilegen-v0.1.0...martin-tilegen-v0.1.1) - 2026-10-22

### Other

- *(deps)* update mlt-core to 0.22.0 ([#3712](https://github.com/maplibre/martin/pull/3712))
