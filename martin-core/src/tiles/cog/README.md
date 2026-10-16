## COG tile sources

`CogSource` serves `WebMercatorQuad` tiles from Cloud Optimized `GeoTIFF`s using the
[`cog-tiles`](https://crates.io/crates/cog-tiles) crate, from one file, local or in any `object_store`.

Each image in a COG (the full-resolution image and each overview) serves one zoom level, and each
served tile is exactly one internal tile of that image, so nothing is resampled:
JPEG and WebP tiles are returned as stored (with `JPEGTables` merged for JPEG),
other 8-bit layouts are re-encoded as PNG. Tiles the file does not store, or that lie outside the
image, are empty.

### COG file requirements

* The `ProjectedCRS` `GeoKey` must be 3857 (EPSG:3857), with square pixels and no rotation.
* Every image must have the resolution of a `WebMercatorQuad` zoom level for its tile size (within 0.1%),
  and its internal tiles must line up with that zoom's tiles. Images that do not (e.g. an overview that
  starts part-way into a tile) are skipped with a warning instead of being snapped to the grid.
* Tiles, not strips, all square and of the same size; bands interleaved (`PlanarConfiguration=1`).
* 8-bit gray, gray + alpha, RGB, RGBA, or YCbCr (JPEG) samples, compressed with None, LZW, Deflate, ZSTD, JPEG, or WebP.
* Transparency masks are ignored.

Using GDAL, you can create a compatible COG with:

```bash
gdal_translate original.tif compatible.tif -of COG -co TILING_SCHEME=GoogleMapsCompatible -co COMPRESS=JPEG
```
