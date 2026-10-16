# cog-tiles

[![crates.io version](https://img.shields.io/crates/v/cog-tiles)](https://crates.io/crates/cog-tiles)
[![docs.rs status](https://img.shields.io/docsrs/cog-tiles)](https://docs.rs/cog-tiles)
[![crates.io license](https://img.shields.io/crates/l/cog-tiles)](https://github.com/maplibre/martin/blob/main/LICENSE-APACHE)

Read web map tiles (`z/x/y`, `WebMercatorQuad`) straight from Cloud Optimized `GeoTIFF`s (COGs),
fetching only the bytes each tile needs from local files or any object store (S3, GCS, Azure, HTTP),
ready for a tile server to send. It does for COGs what [`pmtiles`](https://crates.io/crates/pmtiles)
does for `PMTiles` archives. The TIFF parsing itself is done by
[`async-tiff`](https://crates.io/crates/async-tiff).

## Supported files

A COG can be read as tiles when it is already a tile pyramid on the `WebMercatorQuad` grid, which is what
`gdal_translate -of COG -co TILING_SCHEME=GoogleMapsCompatible` produces:

* `EPSG:3857`, with square pixels and no rotation;
* every image (full resolution and each overview) has the resolution of a zoom level, and its
  internal tiles line up with that zoom's tiles (within 0.05 px). Images that do not (e.g. a
  misaligned overview) are left out and reported by `AsyncCogReader::skipped_images`, as long as
  at least one image can be read;
* square internal tiles, 8-bit unsigned samples, pixel-interleaved.

Each tile it returns is then exactly one internal tile, so nothing is resampled:

| Stored as                                                           | Returned as                                  |
|---------------------------------------------------------------------|----------------------------------------------|
| JPEG (YCbCr, RGB, or grayscale)                                     | JPEG, as stored, with `JPEGTables` merged in |
| WebP                                                                | WebP, as stored                              |
| none / Deflate / LZW / ZSTD (gray, gray+alpha, RGB, RGBA), JPEG RGBA | PNG, re-encoded (`png` feature)             |

Files that cannot be read this way are rejected when opened, with an error saying why
(e.g. "its CRS is EPSG:32610", "image 2 is not aligned to the zoom 18 tile grid").
Sparse (unwritten) tiles, tiles outside the image, and zooms without an image return `None`.

## Usage

One file:

```rust,no_run
# #[cfg(feature = "fs")]
# async fn run() -> cog_tiles::CogResult<()> {
use cog_tiles::{AsyncCogReader, FileBackend, TileCoord};

let reader = AsyncCogReader::try_from_backend(FileBackend::open("ortho.tif").await?).await?;
let info = reader.info();
println!("zooms {}-{}, {:?}, bounds {:?}", info.min_zoom, info.max_zoom, info.format, info.bounds);
if let Some(tile) = reader.get_tile(TileCoord::new(18, 41936, 101296)?).await? {
    // A tile server would send `tile` with Content-Type `info.format.content_type()`.
}
# Ok(())
# }
```

The structure follows `pmtiles`: an `AsyncBackend` trait for byte-range sources
(`MemoryBackend`, `FileBackend`, `ObjectStoreBackend`), an `AsyncCogReader` that parses headers
once and is shared by all requests. Mosaics of many COGs are left to the caller, e.g. Martin opens
one reader per file and keeps them in its own cache.

## Features

* `fs` (default): local files via `FileBackend`.
* `png` (default): re-encode non-JPEG/WebP tiles as PNG.
* `object-store`: `ObjectStoreBackend` over any
  [`object_store`](https://crates.io/crates/object_store). Reads are pinned to the object's
  `ETag`/version, so a replaced object fails instead of mixing two files.

## Not supported yet

* Float or 16-bit data (e.g. elevation). A natural next step is encoding elevation as
  Terrarium/Mapbox RGB.
* LERC compression (needs `async-tiff`'s `lerc` feature), transparency masks, and tile grids other
  than `WebMercatorQuad`.
* Resampling or reprojecting COGs that are not tile-aligned. Convert them first with
  `gdalwarp -t_srs EPSG:3857` and `gdal_translate -of COG -co TILING_SCHEME=GoogleMapsCompatible`.

## Development

This crate is part of the [Martin](https://github.com/maplibre/martin) workspace and is used by its COG sources.
Its test COGs are in the workspace's `tests/fixtures/cog/synthetic`, generated (along with Martin's mosaic test files) by `python3 tests/fixtures/cog/synthetic/generate.py` (needs GDAL's Python bindings).

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](../LICENSE-MIT) or <https://opensource.org/licenses/MIT>)
  at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
