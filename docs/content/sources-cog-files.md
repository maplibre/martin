---
description: Serving tiles from local or cloud-hosted GeoTIFF files
icon: material/satellite-variant
tags:
  - cog
  - tile-sources
  - configuration
  - aws
  - azure
  - google-cloud
  - object-storage
---

# Cloud Optimized GeoTIFF File Sources

Martin supports serving raster sources such as local and remote [COG (Cloud Optimized GeoTIFF)](https://cogeo.org/) files.

## How Martin serves a COG

A COG stores one image per zoom level: the full-resolution image and its downsampled overviews.
Each image is cut into internal tiles, so a `z/x/y` request maps onto exactly one of them.

--8<-- "images/sources/cog-zxy.svg"

Martin reads this mapping from the header once.
Each tile is then one range read.

## File requirements

Because every served tile is exactly one internal tile of the file, nothing is resampled, and the file has to be laid out for that:

- The projected CRS must be [`EPSG:3857`](https://epsg.io/3857), with square pixels and no rotation.
- The full-resolution image and every overview must have the resolution of a `WebMercatorQuad` zoom level for their tile size,
  and their internal tiles must line up with that zoom's tiles.
  An image that does not, such as an overview starting part-way into a tile, is skipped with a warning rather than served shifted;
  the file is rejected only if none of its images can be served.
- The image must be stored in square tiles, all of the same size, or in uncompressed strips, with the bands interleaved (`PlanarConfiguration=1`).
  Striped images are cut into 256 px tiles, read row by row from the strips.
- Overviews are reduced-resolution images (`NewSubfileType=1`), or pages (`NewSubfileType=2`) that are each half the size of the image before them.

--8<-- "images/sources/cog-align.svg"

GDAL produces such files with `-co TILING_SCHEME=GoogleMapsCompatible`, see [COG generation with GDAL](#cog-generation-with-gdal).

## Supported formats

| compression              | bands                                  | served as            |
|--------------------------|----------------------------------------|----------------------|
| JPEG                     | 8-bit gray, YCbCr, or RGB              | JPEG, passed through |
| JPEG                     | 8-bit RGBA                             | PNG                  |
| WebP                     | 8-bit RGB or RGBA                      | WebP, passed through |
| None, LZW, Deflate, ZSTD | 8-bit gray, gray + alpha, RGB, or RGBA | PNG                  |
| None, in strips          | 8-bit gray, gray + alpha, RGB, or RGBA | PNG                  |

Tiles the file does not store (sparse files) and tiles outside the image are served as empty responses.

Not supported yet:

- elevation data and other images that are not 8-bit (16-bit or floating point)
- transparency masks, which are ignored

## Run Martin with CLI to serve cog files

```bash
# Configured with a directory containing `*.tif` or `*.tiff` TIFF files.
martin /with/tiff/dir1 /with/tiff/dir2
# Configured with dedicated TIFF files, local or remote.
martin /path/to/target1.tif https://example.org/path/cog.tif
# Configured with a combination of directories and dedicated TIFF files.
martin /with/tiff/files /path/to/target1.tif /path/to/target2.tiff
# Configured with a remote prefix; every TIFF object under it becomes a source.
martin s3://bucket/imagery/
```

## Run Martin with configuration file

To add a COG in martin, simply add

```yml
# Cloud Optimized GeoTIFF File Sources
cog:
  # Interval between remote polls (HEAD checks and prefix re-listings). Defaults to "10m".
  # Set to "0s" to disable remote polling and remote-prefix discovery.
  reload_interval: 10m
  # Authentication, endpoint, and HTTP client settings are documented under "Remote COG" below.
  allow_http: true
  paths:
    # scan this whole dir, matching all *.tif and *.tiff files
    - /dir-path
    # specific TIFF file will be published as a cog source
    - /path/to/cog_file1.tif
    - /path/to/cog_file2.tiff
    # every TIFF object under this remote prefix becomes a cog source
    - s3://my-bucket/imagery/
  sources:
    # named source matching source name to a single file, local or remote
     cog-src1: /path/to/cog1.tif
     cog-src2: https://example.org/path/cog2.tif
  # mosaics of many files, see "Mosaic of many COGs" below
  mosaics:
    ortho:
      path: s3://my-bucket/ortho/{quadkey}.tif
      zoom: 16
```

!!! tip "Scanning subdirectories"
    Set `recursive: true` next to `paths` to scan subdirectories too.
    A nested file is named by its path relative to the scanned directory with `/` replaced by `.`, so `2024/roads.tif` becomes `2024.roads`.

!!! tip "Per-project directories"
    List a directory of project directories under `collections` to publish every file inside a project as `<project>.<file>`, so `/projects/tiles/project1/elevation.tif` becomes `project1.elevation`.

## Mosaic of many COGs

Large imagery is often cut into many COGs, one per tile at some zoom level, and named after that tile.
A mosaic source serves all of them as one source, without listing or indexing the files:
a request for `z/x/y` is routed to the file of its ancestor tile at the mosaic's `zoom`.

--8<-- "images/sources/cog-mosaic.svg"

```yaml
cog:
  mosaics:
    ortho:
      # Where the files are, local or remote, naming each file by its tile at `zoom`:
      # {quadkey}, or {x} and {y} with an optional {z}, e.g. /data/ortho/{z}/{x}/{y}.tif.
      # Use {-y} for rows counted from the bottom (TMS), e.g. /data/quads/{x}-{-y}.tif.
      path: s3://my-bucket/ortho/{quadkey}.tif
      # The zoom level at which each file covers exactly one tile. It is also the lowest zoom served.
      zoom: 16
      # The highest zoom served. Defaults to the highest zoom of the file opened at startup.
      maxzoom: 20
      # How many opened files (and known-missing ones) are kept at once. Defaults to 1000.
      max_cached_files: 1000
      # How long an opened file is kept before it is opened again. Defaults to "10m".
      file_expiry: 10m
      # How long an opened file is kept without being used. Defaults to none: kept until `file_expiry`.
      file_idle_timeout: 5m
      # Like a source, a mosaic also takes `cache`, `cache_control`, `convert_to_hillshade`, and `convert_to_contour`.
```

--8<-- "images/sources/cog-mosaic-names.svg"

- Every file must meet the [file requirements](#file-requirements), cover its tile at `zoom`, and use the same tile size and format as the others.
  A file that does not is reported as an error for the tiles it would serve.
- A file without GeoTIFF tags is placed exactly on the tile its name stands for, at the resolution its width gives.
- At startup, Martin lists the location until it finds one file whose name matches the template, and opens it to learn the tile size and format.
  For a remote mosaic this needs permission to list the bucket prefix.
- Other files are opened on first use: a `HEAD` request and one read of the header (or one per IFD, when IFDs follow the pixels), kept for `file_expiry`.
  Tiles whose file does not exist, below `zoom`, or beyond the files' own zoom range are served as empty responses.
- Mosaics are not watched or polled: replaced and added files are picked up once their cache entry expires.

--8<-- "images/sources/cog-mosaic-zooms.svg"

## COG Hot Reload

Martin picks up changed COGs without a restart, except for local files listed one by one.
How quickly depends on where the COG is:

--8<-- "images/sources/cog-reload.svg"

### Local directories

Directories in `cog.paths` are watched with filesystem events:

- **File added** - the new source appears in the catalog.
- **File modified** - the source is reloaded and its tile cache is invalidated.
- **File removed** - the source is removed from the catalog.

Local files listed directly in `cog.paths` or `cog.sources` are loaded at startup and not watched.

### Remote files and prefixes

Object stores and HTTP(S) servers send no events, so Martin polls them every `cog.reload_interval` (default `10m`):

- **Remote files** - URLs in `cog.paths`, `cog.sources`, or on the CLI are loaded at startup.
  At each poll a `HEAD` request checks them, and a source is rebuilt when its `ETag` or `Last-Modified` changes.
- **Remote prefixes** - prefixes in `cog.paths` are first listed when polling starts, and listed again at each poll.
  Each listing is compared with the last one, so added, replaced, and removed TIFF objects are added, rebuilt, and removed.

Setting `reload_interval` to `0s` turns polling off: remote files still load at startup but are not checked again,
and remote prefixes are never listed, so their sources never load.

If a `HEAD` request or prefix listing fails, Martin keeps serving what it had, so a short outage does not remove live sources.
A prefix that has not been listed successfully yet is retried at the next poll.
With `on_invalid: warn`, a file that fails to load is also retried at the next poll; a replacement that fails to load leaves the previous version in place.

### Mosaics

Mosaics are not watched or polled.
Each file is kept open for `file_expiry` and opened again on its next use, which picks up replaced and added files.
A file unused for `file_idle_timeout` is closed early and also opened again on its next use.

## Remote COG

Remote COGs are read with byte-range requests, so Martin fetches the TIFF metadata and image chunks it needs instead of downloading the complete object first.
Only `.tif` and `.tiff` objects under a listed prefix are published, using the file stem as the initial source ID.
An object that is also configured under `cog.sources` is skipped by the listing, so it is served once, under its configured ID and with its own settings.

```yaml
cog:
  reload_interval: 1m
  endpoint: http://localhost:9000
  region: us-east-1
  allow_http: true
  access_key_id: ${AWS_ACCESS_KEY_ID}
  secret_access_key: ${AWS_SECRET_ACCESS_KEY}
  paths:
    - s3://my-bucket/imagery/
  sources:
    mosaic: s3://my-bucket/imagery/mosaic.tif
    raster: https://tiles.example.org/mosaic.tif
```

--8<-- "object-store/configuration.md"

## About COG

[COG](https://cogeo.org/) is just Cloud Optimized GeoTIFF file.

TIFF is an image file format.
TIFF tags are something like key-value pairs inside to describe the metadata about a TIFF file, ike `ImageWidth`, `ImageLength`, etc.

GeoTIFF is a valid TIFF file with a set of TIFF tags to describe the 'Cartographic' information associated with it.

COG is a valid GeoTIFF file with some requirements for efficient reading.
That is, all COG files are valid GeoTIFF files, but not all GeoTIFF files are valid COG files.
For quick access to tiles in TIFF files, Martin relies on the requirements/recommendations(like the [requirement about Reduced-Resolution Subfiles](https://docs.ogc.org/is/21-026/21-026.html#_requirement_reduced_resolution_subfiles) and [the content dividing strategy](https://docs.ogc.org/is/21-026/21-026.html#_tiles)) so we use the term `COG` over `GeoTIFF` in our documentation and configuration files.

You may want to visit these specs:

- [TIFF 6.0](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf)
- [GeoTIFF](https://docs.ogc.org/is/19-008r4/19-008r4.html)
- [Cloud Optimized GeoTIFF](https://docs.ogc.org/is/21-026/21-026.html)

### COG generation with GDAL

You could generate cog with `gdal_translate` or `gdalwarp`. See more details in [gdal doc](https://gdal.org/en/latest/drivers/raster/cog.html).

```bash
# gdal-bin installation
# sudo apt update
# sudo apt install gdal-bin

# gdalwarp, warping to Web Mercator and aligning every level to the tile grid
gdalwarp src1.tif src2.tif out.tif -of COG -co TILING_SCHEME=GoogleMapsCompatible

# or gdal_translate
gdal_translate input.tif output_cog.tif -of COG -co TILING_SCHEME=GoogleMapsCompatible
```
