# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/maplibre/martin/compare/martin-tilegen-v0.3.0...martin-tilegen-v0.3.1) - 2026-11-28

### Other

- format the justfiles and keep them formatted with a prek hook ([#4100](https://github.com/maplibre/martin/pull/4100))
- *(expr)* compile each matches() pattern once per thread ([#4082](https://github.com/maplibre/martin/pull/4082))
- *(osm)* look up condition columns by name in a hash map ([#4084](https://github.com/maplibre/martin/pull/4084))
- *(render)* skip zooms whose kept tiles a feature cannot reach, before simplifying ([#4085](https://github.com/maplibre/martin/pull/4085))
- *(expr)* evaluate `x in [literals]` without the interpreter ([#4083](https://github.com/maplibre/martin/pull/4083))
- *(nightly)* ship `--format mlt2` in the nightlies and update mlt-core to 0.24.0 ([#4073](https://github.com/maplibre/martin/pull/4073))

## [0.3.0](https://github.com/maplibre/martin/compare/martin-tilegen-v0.2.1...martin-tilegen-v0.3.0) - 2026-11-25

### Added

- OpenStreetMap PBF sources, with closed ways read as Planetiler reads them and relation membership through named relation sets ([#3954](https://github.com/maplibre/martin/pull/3954), [#3967](https://github.com/maplibre/martin/pull/3967), [#3977](https://github.com/maplibre/martin/pull/3977))
- ESRI shapefile sources, rendered in one run with `osm` sources through `SourceChain` ([#3971](https://github.com/maplibre/martin/pull/3971), [#3972](https://github.com/maplibre/martin/pull/3972))
- The Shortbread and OpenMapTiles configurations ([#3956](https://github.com/maplibre/martin/pull/3956), [#3978](https://github.com/maplibre/martin/pull/3978))
- `layer` writes several layer definitions into one output layer ([#3973](https://github.com/maplibre/martin/pull/3973))
- `geometry` as a list, `polygon_point_on_surface` and `polygon_centroid_if_convex` ([#3968](https://github.com/maplibre/martin/pull/3968), [#3969](https://github.com/maplibre/martin/pull/3969), [#3970](https://github.com/maplibre/martin/pull/3970))
- `area(unit)`, `matches`, `regex.replace`, `min`, `isLatin` and `removeLatin` in layer expressions ([#3966](https://github.com/maplibre/martin/pull/3966), [#3974](https://github.com/maplibre/martin/pull/3974))
- Grid thinning `rank` and `hidden` attributes ([#3975](https://github.com/maplibre/martin/pull/3975), [#3976](https://github.com/maplibre/martin/pull/3976))
- `jsonb` documents as nested columns in MLT v2 tiles ([#4026](https://github.com/maplibre/martin/pull/4026), [#4027](https://github.com/maplibre/martin/pull/4027), [#4029](https://github.com/maplibre/martin/pull/4029), [#4030](https://github.com/maplibre/martin/pull/4030))

### Fixed

- One MLT column type per key in every tile of a layer, a warning for rounded integers, and encode errors naming their layer and tile ([#3982](https://github.com/maplibre/martin/pull/3982), [#3983](https://github.com/maplibre/martin/pull/3983))
- Tables without `layers` follow `order_by`, and the table comment metadata and `compression` are kept ([#3942](https://github.com/maplibre/martin/pull/3942), [#3944](https://github.com/maplibre/martin/pull/3944))

## [0.2.1](https://github.com/maplibre/martin/compare/martin-tilegen-v0.2.0...martin-tilegen-v0.2.1) - 2026-11-11

### Added

- *(generate)* draw `polygon_centroid`, `line_centroid` and `centroid` as their centroid point ([#3920](https://github.com/maplibre/martin/pull/3920))
- *(generate)* add the `merge_polygons` per-tile operation ([#3907](https://github.com/maplibre/martin/pull/3907))
- *(generate)* add the `merge_lines` per-tile operation ([#3906](https://github.com/maplibre/martin/pull/3906))
- *(generate)* add `thinning` keys and the `distance` and `tile` methods ([#3905](https://github.com/maplibre/martin/pull/3905))
- *(generate)* add the `merge_multi` per-tile operation ([#3904](https://github.com/maplibre/martin/pull/3904))
- *(generate)* add the `thinning` per-tile operation with `method: grid` ([#3903](https://github.com/maplibre/martin/pull/3903))
- *(generate)* add per-tile layer operations, starting with `limit` ([#3902](https://github.com/maplibre/martin/pull/3902))
- *(cp)* show stages and busy threads while `--mode bulk` runs ([#3911](https://github.com/maplibre/martin/pull/3911))

### Fixed

- fmt ([#3892](https://github.com/maplibre/martin/pull/3892))

## [0.2.0](https://github.com/maplibre/martin/compare/martin-tilegen-v0.1.2...martin-tilegen-v0.2.0) - 2026-11-04

### Added

- *(mbtiles)* [**breaking**] warn instead of failing on a missing unique tile index ([#3871](https://github.com/maplibre/martin/pull/3871))

### Other

- *(deps)* drop the wkb git patch so the workspace publishes ([#3853](https://github.com/maplibre/martin/pull/3853))

## [0.1.2](https://github.com/maplibre/martin/compare/martin-tilegen-v0.1.1...martin-tilegen-v0.1.2) - 2026-10-29

### Added

- *(tilegen)* MVT polygon rings start where ST_AsMVTGeom's wagyu does, `--mvt-quirk preserve-ring-start` keeps the source start ([#3812](https://github.com/maplibre/martin/pull/3812))
- *(tilegen)* `martin cp --mode bulk --mvt-declared-keys` lists every declared key, as ST_AsMVT does ([#3811](https://github.com/maplibre/martin/pull/3811))
- *(martin)* `martin test generate` and `run` over real rows ([#3686](https://github.com/maplibre/martin/pull/3686))
- *(martin)* `martin test` paths for lines crossing a tile edge or shorter than a tile unit ([#3697](https://github.com/maplibre/martin/pull/3697))
- *(martin)* a `martin test` path per value the source's filter allows ([#3696](https://github.com/maplibre/martin/pull/3696))
- *(tilegen)* list the paths a feature can take through a layer ([#3685](https://github.com/maplibre/martin/pull/3685))

### Fixed

- *(tilegen)* pick tiles for points and unclipped features from the source geometry, as tile mode does ([#3780](https://github.com/maplibre/martin/pull/3780))
- *(tilegen)* simplification keeps the first of equally far vertices, as PostGIS does ([#3779](https://github.com/maplibre/martin/pull/3779))
- *(tilegen)* bulk mode keeps what lies on a tile's buffer edge, as tile mode does ([#3767](https://github.com/maplibre/martin/pull/3767))
- *(tilegen)* bulk mode rounds halves to even, as tile mode does ([#3766](https://github.com/maplibre/martin/pull/3766))
- *(tilegen)* `martin test` has no paths that `NOT NULL`, enum labels, a `CHECK` or the key rule out ([#3749](https://github.com/maplibre/martin/pull/3749))
- *(tilegen)* MultiPolygon parts that round onto each other merge instead of overlapping ([#3752](https://github.com/maplibre/martin/pull/3752))
- *(tilegen)* stop on geometries a tile cannot hold, naming the feature ([#3756](https://github.com/maplibre/martin/pull/3756))
- *(tilegen)* keep a multipoint whose parts include an EMPTY point ([#3751](https://github.com/maplibre/martin/pull/3751))
- *(generate)* keep unclipped features whole in every tile any part reaches ([#3731](https://github.com/maplibre/martin/pull/3731))
- *(generate)* drop vertices rounded onto a straight run ([#3716](https://github.com/maplibre/martin/pull/3716))

### Other

- *(tilegen)* evaluate comparisons and boolean logic without the CEL interpreter ([#3830](https://github.com/maplibre/martin/pull/3830))
- *(generate)* run the pushdown and engine-serve PG tests, cover untested bulk paths ([#3739](https://github.com/maplibre/martin/pull/3739))
- *(toolkit)* share Quantizer's edge tests and ring helpers ([#3723](https://github.com/maplibre/martin/pull/3723))

## [0.1.1](https://github.com/maplibre/martin/compare/martin-tilegen-v0.1.0...martin-tilegen-v0.1.1) - 2026-10-22

### Other

- *(deps)* update mlt-core to 0.22.0 ([#3712](https://github.com/maplibre/martin/pull/3712))
