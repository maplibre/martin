# martin-tilegen

[![docs.rs docs](https://docs.rs/martin-tilegen/badge.svg)](https://docs.rs/martin-tilegen)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)

Source-neutral bulk tile generation engine used by `martin generate`.
Each feature is read once, rendered for every zoom, and its tile pieces are spilled as sorted runs keyed by
`(tile, layer, source order)`. A k-way merge then brings each tile's pieces together so the tile can be
encoded as MLT or MVT and written to an archive in the archive's preferred order.

The design is inspired by [Planetiler](https://github.com/onthegomap/planetiler), written by Mike Barry
([@msbarry](https://github.com/msbarry)) and licensed under the
[Apache License 2.0](https://github.com/onthegomap/planetiler/blob/main/LICENSE). Thank you to him and the
Planetiler contributors. We built it because `martin cp`, which asks the source for each tile separately,
does not scale to large tilesets.
