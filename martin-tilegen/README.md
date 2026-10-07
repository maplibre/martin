# martin-tilegen

[![docs.rs docs](https://docs.rs/martin-tilegen/badge.svg)](https://docs.rs/martin-tilegen)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)

Source-neutral bulk tile generation engine used by `martin generate`.
Each feature is read once, rendered for every zoom, and its tile pieces are spilled as sorted runs keyed by
`(tile, layer, source order)`. A k-way merge then brings each tile's pieces together so the tile can be
encoded as MLT or MVT and written to an archive in the archive's preferred order.
