# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/maplibre/martin/compare/cog-tiles-v0.1.1...cog-tiles-v0.1.2) - 2026-10-22

### Added

- `mosaic` feature: read mosaics of many COGs, each named after the tile it covers ([#3718](https://github.com/maplibre/martin/pull/3718)).
- Decode tiles to pixels and stitch them into windows; `webp` feature decodes WebP tiles ([#3719](https://github.com/maplibre/martin/pull/3719)).

## [0.1.1](https://github.com/maplibre/martin/compare/cog-tiles-v0.1.0...cog-tiles-v0.1.1) - 2026-10-22

### Other

- update Cargo.toml dependencies

### Added

- `AsyncCogReader`: read `WebMercatorQuad` tiles from one tile-aligned COG, passing JPEG/WebP tiles
  through and re-encoding lossless 8-bit tiles as PNG.
- Backends: `MemoryBackend`, `FileBackend` (`fs`), `ObjectStoreBackend` (`object-store`).
- `get_tile_pixels` on readers and mosaics: decoded 8-bit tiles as `TilePixels`; WebP needs the `webp` feature.
- `PixelWindow`: the tiles an area touches at one zoom, stitched into one raster with its geotransform.
- `lnglat_to_mercator` and `MAX_LATITUDE`.
- `AsyncCogMosaic` (`mosaic`): read many COGs as one, each named after its tile by a `KeyTemplate`
  (e.g. `{quadkey}.tif`), moved from `martin-core`.
