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
* square internal tiles or uncompressed strips, 8-bit unsigned samples, pixel-interleaved.
  Striped images are split into 256 px tiles;
* overviews are reduced-resolution subfiles, or pages (`NewSubfileType=2`) half the size of the previous page.

In a mosaic, square files without a `GeoKeyDirectory` are placed on the tile in their file name.

Each tile it returns is then exactly one internal tile (or a 256 px block of strips), so nothing is resampled:

| Stored as                                                           | Returned as                                  |
|---------------------------------------------------------------------|----------------------------------------------|
| JPEG (YCbCr, RGB, or grayscale)                                     | JPEG, as stored, with `JPEGTables` merged in |
| WebP                                                                | WebP, as stored                              |
| none / Deflate / LZW / ZSTD (gray, gray+alpha, RGB, RGBA), JPEG RGBA | PNG, re-encoded (`png` feature)             |
| uncompressed strips (gray, gray+alpha, RGB, RGBA)                   | PNG, re-encoded (`png` feature)              |

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
once and is shared by all requests.

Many files, each covering one tile at a file zoom and named after it (`mosaic` feature):

```rust,no_run
# #[cfg(feature = "mosaic")]
# async fn run() -> cog_tiles::CogResult<()> {
use cog_tiles::{AsyncCogMosaic, MosaicOptions, TileCoord};

let mosaic = AsyncCogMosaic::open_dir("/data/ortho", "{quadkey}.tif", MosaicOptions::new(16)).await?;
let tile = mosaic.get_tile(TileCoord::new(18, 130_992, 87_176)?).await?;
# Ok(())
# }
```

Files in a UTM zone or on any pixel grid, found per tile by an index of your own (`mosaic` feature).
Each tile is painted from the files the index returns, in order, reprojected with nearest-neighbor resampling.
Later files only fill the pixels earlier ones leave empty (nodata, or alpha 0).
A tile is read from the coarsest overview at least as detailed as the tile, and refused when even that is more than 4 times as detailed, so files need overviews down to the mosaic's min zoom:

```rust,no_run
# #[cfg(feature = "mosaic")]
# async fn run(index: std::sync::Arc<dyn cog_tiles::MosaicIndex>, first: cog_tiles::MosaicFile) -> cog_tiles::CogResult<()> {
use cog_tiles::{AsyncIndexedMosaic, IndexedMosaicOptions, TileCoord};

let options = IndexedMosaicOptions::new(12, vec![0, 1, 2]).with_nodata(Some(0));
let mosaic = AsyncIndexedMosaic::open(index, &first, options).await?;
let tile = mosaic.get_tile(TileCoord::new(16, 10_477, 25_326)?).await?;
# Ok(())
# }
```

Two indexes are built in. A `gdaltindex` tile index (`tile-index` feature) is read into memory once;
a STAC API (`stac` feature) is searched once per tile at `search_zoom`, its results cached:

```rust,no_run
# #[cfg(all(feature = "stac", feature = "tile-index"))]
# async fn run(resolve: cog_tiles::HrefResolver, search: cog_tiles::StacSearchOptions) -> cog_tiles::CogResult<()> {
use cog_tiles::{AsyncIndexedMosaic, DEFAULT_LOCATION_FIELD, IndexedMosaicOptions, StacIndex, TileIndex};

let index = TileIndex::open("naip.gpkg".as_ref(), None, DEFAULT_LOCATION_FIELD, &resolve).await?;
let naip = AsyncIndexedMosaic::open_tile_index(index, IndexedMosaicOptions::new(12, vec![0, 1, 2])).await?;

let index = StacIndex::open(search, None, resolve).await?;
let options = IndexedMosaicOptions::new(13, index.bands().to_vec());
let stac = AsyncIndexedMosaic::open_stac(index, options).await?;
# Ok(())
# }
```

Decoded pixels instead of encoded tiles, e.g. to stitch an area into one raster:

```rust,no_run
# async fn run(reader: cog_tiles::AsyncCogReader<cog_tiles::MemoryBackend>) -> cog_tiles::CogResult<()> {
use cog_tiles::PixelWindow;

let window = PixelWindow::from_lnglat_bounds(18, reader.info().tile_size, [-0.1, 51.49, -0.09, 51.5])?;
let mut raster = None;
for coord in window.tiles() {
    if let Some(pixels) = reader.get_tile_pixels(coord).await? {
        let size = window.width() as usize * window.height() as usize * pixels.bands();
        window.paste(raster.get_or_insert_with(|| vec![0; size]), coord, &pixels);
    }
}
// `window.geotransform()` places `raster` in EPSG:3857.
# Ok(())
# }
```

A `KeyTemplate` names each file by its tile at the file zoom with `{quadkey}`, `{z}`, `{x}`, `{y}`
and `{-y}` (the row counted from the bottom, as in TMS). Files are opened on first use and kept in
a bounded, expiring cache; tiles without a file are `None`.

## Features

* `fs` (default): local files via `FileBackend`.
* `png` (default): re-encode non-JPEG/WebP tiles as PNG.
* `object-store`: `ObjectStoreBackend` over any
  [`object_store`](https://crates.io/crates/object_store). Reads are pinned to the object's
  `ETag`/version, so a replaced object fails instead of mixing two files.
* `webp`: decode WebP tiles with `get_tile_pixels` (links `libwebp`). Without it, WebP tiles are only passed through.
* `mosaic`: `AsyncCogMosaic` over a local directory or an object store prefix, and
  `AsyncIndexedMosaic` over the files a `MosaicIndex` finds (enables `fs`, `png` and `object-store`).
* `tile-index`: `TileIndex`, the files of a `gdaltindex` `GeoPackage` (enables `mosaic`).
* `stac`: `StacIndex`, the items a STAC API finds (enables `mosaic`).

## Not supported yet

* Float or 16-bit data (e.g. elevation). A natural next step is encoding elevation as
  Terrarium/Mapbox RGB.
* LERC compression (needs `async-tiff`'s `lerc` feature), transparency masks, and tile grids other
  than `WebMercatorQuad`.
* Reprojecting a single COG outside a mosaic, CRSs other than Web Mercator and UTM, and resampling
  other than nearest-neighbor. Convert such files first with `gdalwarp -t_srs EPSG:3857` and
  `gdal_translate -of COG -co TILING_SCHEME=GoogleMapsCompatible`.

## Development

This crate is part of the [Martin](https://github.com/maplibre/martin) workspace and is used by its COG sources.
Its test COGs are in the workspace's `tests/fixtures/cog/synthetic`, generated (along with Martin's mosaic test files) by `python3 tests/fixtures/cog/synthetic/generate.py` (needs GDAL's Python bindings).
The reprojection fixtures in `tests/fixtures/cog/warped`, and the `gdalwarp` tiles they are checked against, come from `python3 tests/fixtures/cog/warped/generate.py` (needs numpy and the GDAL command line tools).

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
