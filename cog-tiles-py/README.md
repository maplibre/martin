# cog-tiles for Python

Web map tiles and stitched rasters from tile-aligned Cloud Optimized GeoTIFFs (COGs), local or in
S3, GCS, Azure or over HTTP, fetching only the bytes each tile needs.
Python bindings for the Rust [`cog-tiles`](https://crates.io/crates/cog-tiles) crate used by
[Martin](https://github.com/maplibre/martin).

```python
import cog_tiles

cog = cog_tiles.open("s3://bucket/ortho.tif", options={"aws_region": "us-west-2"})
cog.min_zoom, cog.max_zoom, cog.tile_size, cog.format, cog.bounds

# One encoded tile, ready to serve or store (JPEG/WebP as stored, others as PNG).
png = cog.tile(18, 41936, 101296)

# Every tile in an area, fetched 64 at a time in the background, in arrival order.
for z, x, y, data in cog.tiles(bbox=(-122.42, 37.77, -122.41, 37.78), zoom=range(16, 19)):
    ...

# An area stitched into one (height, width, bands) uint8 array, decoded on all cores.
raster = cog.read((-122.42, 37.77, -122.41, 37.78), zoom=18)
raster.data.shape     # (1311, 1166, 3)
raster.transform      # GDAL geotransform in EPSG:3857
```

![What tile, tiles and read return](https://raw.githubusercontent.com/maplibre/martin/main/cog-tiles-py/docs/images/tile-tiles-read.svg)

Many files, each covering one tile at a file zoom and named after it:

```python
mosaic = cog_tiles.open_mosaic("s3://bucket/ortho/{quadkey}.tif", file_zoom=12)
raster = mosaic.read(bbox, zoom=18)
```

Templates use `{quadkey}`, or `{x}` and `{y}` with an optional `{z}` (`{-y}` counts rows from the
bottom, as in TMS). One file is found by listing the location when it is opened; the others are
opened on first use and cached.

## What it reads

COGs on the `WebMercatorQuad` grid, as `gdal_translate -of COG -co TILING_SCHEME=GoogleMapsCompatible`
writes them: EPSG:3857, every image (and overview) at a zoom level's resolution and aligned to its
tiles, 8-bit samples. Each tile is one internal tile, so nothing is resampled. Other files raise
`UnsupportedCogError` saying why.

## Performance

* All I/O and decoding runs in Rust with the GIL released; `read` and `tiles` keep `concurrency`
  requests in flight (64 by default) and decode on every core.
* Headers are read once when opening (one request for a typical file); each tile is one range read.
* `tile_pixels` and `read` hand their buffer to NumPy without copying.
* Ctrl-C interrupts any call. Dropping a `tiles()` iterator cancels its pending fetches.

## Details

* `bbox` is `(west, south, east, north)` in degrees, or `(min_x, min_y, max_x, max_y)` in meters
  with `crs="EPSG:3857"`. A raster covers the whole pixels the bbox touches; `raster.bounds` is
  the snapped extent. Pixels without data are 0, and a bbox without any data returns `None`.
* `read` refuses rasters over `max_pixels` (16384 x 16384 by default).
* `Cog` and `Mosaic` objects pickle by reopening, so they work in `multiprocessing` and PyTorch
  `DataLoader` workers. Forked processes get their own I/O threads.
* Object store credentials come from the usual environment variables (`AWS_*`, `GOOGLE_*`,
  `AZURE_*`); `options` overrides them with
  [`object_store` keys](https://docs.rs/object_store/latest/object_store/aws/enum.AmazonS3ConfigKey.html),
  e.g. `{"aws_skip_signature": "true"}` for public buckets.
* Errors: `FileNotFoundError`, `ValueError` for invalid arguments, `UnsupportedCogError` for files
  that cannot be read as tiles, `CogTilesError` for everything else.

## Development

```bash
cd cog-tiles-py
uv venv && uv pip install maturin pytest numpy
uv run maturin develop --release
uv run pytest
```

`cog_tiles.pyi` is generated from the bindings by `just sync-cog-py-stubs`; autofix.ci regenerates it on pull requests.
