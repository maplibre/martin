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
- The image must be stored in square tiles of one size, or in uncompressed strips, with interleaved bands (`PlanarConfiguration=1`) and `Orientation=1`.
  Striped images are split into 256 px tiles.
- Overviews are reduced-resolution images (`NewSubfileType=1`), or pages (`NewSubfileType=2`) half the size of the previous page.
  Other pages are skipped with a warning.

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
- Files without GeoTIFF tags must be square, and are placed on the tile in their file name.
- At startup, Martin lists the location until it finds one file whose name matches the template, and opens it to learn the tile size and format.
  For a remote mosaic this needs permission to list the bucket prefix.
- Other files are opened on first use with a `HEAD` request and one header read (one per IFD if IFDs come after the pixel data), and kept for `file_expiry`.
  Tiles whose file does not exist, below `zoom`, or beyond the files' own zoom range are served as empty responses.
- Mosaics are not watched or polled: replaced and added files are picked up once their cache entry expires.

--8<-- "images/sources/cog-mosaic-zooms.svg"

## Imagery from a STAC API

Imagery catalogs such as [Earth Search](https://earth-search.aws.element84.com/v1) publish each scene as a STAC item, in its own UTM zone and on its own pixel grid, overlapping its neighbors and earlier scenes of the same place.
`cog.stac` serves such a collection as one source: for each tile, Martin searches the STAC API for the items over it and paints them onto the tile, reprojected to Web Mercator.

```yaml
cog:
  # Settings for reading the assets, shared with every other COG source.
  # The NAIP bucket is requester-pays, so requests must say who pays.
  aws_request_payer: true
  aws_region: us-west-2
  stac:
    naip:
      # The root of the STAC API. Items are searched at its `/search` endpoint.
      url: https://earth-search.aws.element84.com/v1
      # The collection to search.
      collection: naip
      # The key of the asset read from each item.
      asset: image
      # The bands to serve, by number (counted from 1) or by `eo:bands` name: one for gray, three for RGB.
      # When unset, [red, green, blue], or the first three bands if the asset does not name its bands.
      bands: [red, green, blue]
      # A STAC `datetime` the items must fall in. When unset, any time.
      datetime: 2018-01-01T00:00:00Z/..
      # A STAC query extension filter, sent as is. When unset, none.
      query:
        naip:state:
          eq: ca
      # Which items cover a pixel first: `newest` or `oldest`. When unset, `newest`.
      order: newest
      # The lowest zoom served. Lower zooms are empty.
      minzoom: 12
      # The highest zoom served. When unset, the zoom at which the first item found is fully detailed.
      maxzoom: 19
      # Each search covers one tile at this zoom, and serves every tile below it. When unset, `minzoom`.
      search_zoom: 12
      # How long a search result is kept before searching again. When unset, "1d".
      search_expiry: 1d
      # How many search results are kept at once. When unset, 10000.
      max_cached_searches: 10000
      # How many items one search reads, counting those without the asset; the rest are not painted and a warning is logged. When unset, 1000.
      max_items_per_search: 1000
      # The sample value of pixels without data, overriding each file's own nodata value. When unset, the file's.
      nodata: 0
      # How many opened files are kept at once, and for how long. Same as for a mosaic.
      max_cached_files: 1000
      file_expiry: 10m
      file_idle_timeout: 5m
      # Rendered tiles are only kept in the tile cache when `cache` is set, as it does not expire with `search_expiry`;
      # pair it with the top-level `cache.expiry`. Like a source, a STAC source also takes `cache_control`.
      cache:
        minzoom: 12
```

- At startup, Martin reads the collection for its extent and band names, and opens the first item that matches the filters to check that it can be served.
  A source that cannot is reported like any other invalid source.
- One search covers one tile at `search_zoom`, and is kept for `search_expiry`: every tile below it is served from that one search.
  Each search follows the API's `next` links until it has read `max_items_per_search` items, sorted by `datetime` (and `id`, so pages do not skip items of the same `datetime`).
- A tile is painted from the items over it in `order`.
  A pixel takes the first item that has data for it, so later items only fill what earlier ones leave empty (outside their extent, or `nodata` in all served bands); painting stops once every pixel is set.
  With `order: newest`, every pixel shows its most recent scene.
- Tiles are 256 px PNGs, transparent where no item has data. Tiles no item covers are served as empty responses.
- Files are reprojected with nearest-neighbor resampling, from the coarsest overview that is still at least as detailed as the tile.
  A tile more than 4 times coarser than that overview is refused, so files need overviews down to `minzoom`.
  A pixel whose alpha band is 0 counts as empty, like `nodata`.
  They must be tiled COGs with 8-bit samples stored pixel-interleaved, in Web Mercator or a UTM zone (EPSG:326xx, 327xx, 269xx, or 258xx).
  `NAD83` and `ETRS89` coordinates are read as `WGS84` ones, without a datum shift.
- Asset `href`s are read with the `cog` object-store settings, so `s3://`, `gs://`, `az://` and `https://` assets all work.

## Imagery listed by a tile index

A tile index is a GeoPackage with one row per COG: the file's footprint and its location.
`cog.tile_index` serves the files it lists as one source, reprojected to Web Mercator.
GDAL's `gdaltindex` writes one from any set of files, local or remote:

```bash
# Header reads only: one small range request per file.
export AWS_REQUEST_PAYER=requester
aws s3 ls --recursive --request-payer requester s3://naip-visualization/ca/2020/60cm/rgb/ \
  | awk '/\.tif$/ {print "/vsis3/naip-visualization/" $4}' > files.txt
gdaltindex -f GPKG -t_srs EPSG:4326 -src_srs_name src_srs naip_ca_2020.gpkg --optfile files.txt
```

```yaml
cog:
  # Settings for reading the files, shared with every other COG source.
  aws_request_payer: true
  aws_region: us-west-2
  tile_index:
    naip:
      # The GeoPackage tile index.
      path: /data/naip_ca_2020.gpkg
      # The feature table to read. When unset, the only feature table in the file.
      layer: naip_ca_2020
      # The column holding each file's location. When unset, `location`, as `gdaltindex` writes it.
      location_field: location
      # The bands to serve, by number (counted from 1): one for gray, three for RGB. When unset, [1, 2, 3].
      bands: [1, 2, 3]
      # The lowest zoom served. Lower zooms are empty.
      minzoom: 16
      # The highest zoom served. When unset, the zoom at which the first file is fully detailed.
      maxzoom: 19
      # The sample value of pixels without data, overriding each file's own nodata value. When unset, the file's.
      nodata: 0
      # How many opened files are kept at once, and for how long. Same as for a mosaic.
      max_cached_files: 1000
      file_expiry: 10m
      file_idle_timeout: 5m
```

- At startup, Martin reads the whole index into memory and opens its first file to check that it can be served.
  A source that cannot is reported like any other invalid source.
- The footprints must be in `EPSG:4326`, as `gdaltindex -t_srs EPSG:4326` writes them; each file may be in its own projection.
- Locations may be URLs (`s3://`, `gs://`, `az://`, `https://`), GDAL `/vsis3/`, `/vsigs/`, `/vsiaz/` or `/vsicurl/` paths, or local paths, relative to the index file unless absolute.
  They are read with the `cog` object-store settings.
- A tile is painted from the files whose footprints touch it, in the index's row order: a pixel takes the first file that has data for it, so later files only fill what earlier ones leave empty.
  To have the newest imagery win, list it first.
- Tiles are 256 px PNGs, transparent where no file has data, and are resampled, reprojected and checked like those of a [STAC source](#imagery-from-a-stac-api).

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
