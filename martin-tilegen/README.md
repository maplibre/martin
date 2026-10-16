# martin-tilegen

[![docs.rs docs](https://docs.rs/martin-tilegen/badge.svg)](https://docs.rs/martin-tilegen)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)

Source-neutral bulk tile generation engine used by `martin cp`.
Each feature is read once, rendered for every zoom, and its tile pieces are spilled as sorted runs keyed by
`(tile, layer, source order)`. A k-way merge then brings each tile's pieces together so the tile can be
encoded as MLT or MVT and written to an archive in the archive's preferred order.

It is part of a long-requested Martin feature: generating tiles on the fly from a feature source when
serving tiles, instead of relying on the source to contain tiles (MBTiles, PMTiles) or generate them
(PostgreSQL's `ST_AsMVT`). Features come from any source that can produce a stream of features.

Moving tile generation out of the database also saves money. Scaling PostgreSQL is significantly harder
and more expensive than scaling stateless Martin instances, for example in Kubernetes. With this engine
the database only streams features, and the CPU-heavy work of clipping, simplifying and encoding tiles
runs in Martin.

The tile generation method is adapted from
[Planetiler](https://github.com/onthegomap/planetiler), written by Mike Barry
([@msbarry](https://github.com/msbarry)) and licensed under the
[Apache License 2.0](https://github.com/onthegomap/planetiler/blob/main/LICENSE). Thank you to him for an
amazing tool, and to the Planetiler contributors.
