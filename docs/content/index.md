---
icon: material/home
hide:
  - navigation
  - toc
---

![Martin](https://raw.githubusercontent.com/maplibre/martin/main/logo.png)

[![docs.rs docs](https://docs.rs/martin/badge.svg)](https://docs.rs/martin)
[![join our community](https://img.shields.io/badge/Slack-%23maplibre--martin-blueviolet?logo=slack)](https://slack.openstreetmap.us/)
[![GitHub](https://img.shields.io/badge/github-maplibre/martin-8da0cb?logo=github)](https://github.com/maplibre/martin)
[![crates.io version](https://img.shields.io/crates/v/martin.svg)](https://crates.io/crates/martin)
[![Security audit](https://github.com/maplibre/martin/workflows/Security%20audit/badge.svg)](https://github.com/maplibre/martin/security)
[![CI build](https://github.com/maplibre/martin/actions/workflows/ci.yml/badge.svg)](https://github.com/maplibre/martin/actions)
[![Codecov](https://img.shields.io/codecov/c/github/maplibre/martin)](https://app.codecov.io/gh/maplibre/martin)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/11613/badge)](https://www.bestpractices.dev/projects/11613)

Martin is a tile server able to generate and serve [vector tiles](https://github.com/mapbox/vector-tile-spec) on the fly from large [PostGIS](https://github.com/postgis/postgis) databases, [PMTiles](https://protomaps.com/blog/pmtiles-v3-whats-new) (local or remote), [MBTiles](https://github.com/mapbox/mbtiles-spec), and [GeoJSON](https://geojson.org/) files, allowing multiple tile sources to be dynamically combined into one.
Martin optimizes for speed and heavy traffic, and is written in [Rust](https://github.com/rust-lang/rust).

# What Martin can do

- Serve [vector tiles](https://github.com/mapbox/vector-tile-spec) from:
  - [PostGIS](https://postgis.net/) databases with automatic discovery of compatible tables and functions
  - [PMTiles](https://docs.protomaps.com/pmtiles/) from local files or over HTTP
  - [MBTiles](https://github.com/mapbox/mbtiles-spec) files
  - [GeoJSON](https://geojson.org/) files
- [Combine](sources-composite.md) multiple tile sources into one
- Serve [styles](sources-styles.md) and generate [sprites](sources-sprites.md) and [font glyphs](sources-fonts.md) on the fly
- Generate tiles in bulk into an MBTiles archive with [martin-cp](martin-cp.md)
- Examine, copy, validate, compare, and apply diffs between MBTiles files with [mbtiles](tools.md#mbtiles)

[Explore on our demo site](https://martin.maplibre.org/){ .md-button .md-button--primary }
