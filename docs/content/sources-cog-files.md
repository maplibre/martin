---
icon: material/satellite-variant
tags:
  - cog
  - tile-sources
  - configuration
---

# Cloud Optimized GeoTIFF File Sources

!!! warning
    This feature is currently unstable and thus not included in the default build.
    Its behavior may change in patch releases.

    To experiment with it, [install Rust](https://rust-lang.org/tools/install/), and run this to download, compile, and install martin with the unstable feature:

    ```bash
    cargo install martin --features=unstable-cog
    ```

    It is unstable due to the limitations of our current implementation:

    - [`EPSG:3857`](https://epsg.io/3857) is not yet supported => <https://github.com/maplibre/martin/pull/1893>

    We welcome contributions to help stabilize this feature!

Martin supports serving raster sources such as local and remote [COG (Cloud Optimized GeoTIFF)](https://cogeo.org/) files.

## Supported color type and bits per sample

| color type | bits per sample | supported | status     |
|------------|-----------------|-----------|------------|
| rgb/rgba   | 8               | ✅        |            |
| rgb/rgba   | 16/32...        | 🛠️        | working on |
| gray       | 8/16/32...      | 🛠️        | working on |

## Supported compression

- None
- LZW
- Deflate
- PackBits

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
  # Authentication, endpoint, and HTTP client settings (see "Remote COG" below).
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
```

## COG Hot Reload

Two mechanisms keep the catalog current at runtime - local directories are watched with filesystem events, remote COGs are polled.

### Local directories

When `.tif` or `.tiff` files are added, modified, or removed from a watched directory, Martin automatically updates the tile catalog - no restart required.

```yaml
cog:
  paths:
    - /path/to/cog/directory
```

!!! tip "Scanning subdirectories"
    Set `recursive: true` next to `paths` to scan subdirectories too.
    A nested file is named by its path relative to the scanned directory with `/` replaced by `.`, so `2024/roads.tif` becomes `2024.roads`.

!!! tip "Per-project directories"
    List a directory of project directories under `collections` to publish every file inside a project as `<project>.<file>`, so `/projects/tiles/project1/elevation.tif` becomes `project1.elevation`.

The following events are handled automatically:

- **File added** - the new source appears in the catalog.
- **File modified** - the source is reloaded and its tile cache is invalidated.
- **File removed** - the source is removed from the catalog.

### Remote COGs

Remote object stores and HTTP(S) servers have no event channel, so Martin polls them at `cog.reload_interval` (default `10m`):

- **Configured remote objects** - `cog.sources` entries with an `s3://`, `gs://`, `az://`, `http://`, or `https://` URL, as well as remote URLs passed on the CLI, are re-checked with a `HEAD` request and rebuilt when their `ETag` or `Last-Modified` changes.
- **Remote prefixes** - prefixes in `cog.paths` are re-listed, and the resulting objects are diffed against the previous snapshot, so added, updated, and removed TIFF objects propagate to the catalog.

Configured remote objects still load at startup when `reload_interval` is `0s`, but are not checked again.
Remote prefixes are first discovered by the polling loop, so setting `reload_interval` to `0s` prevents their sources from loading.

If a later `HEAD` request or prefix listing fails, Martin retains the last-known object version or last successful listing, so a transient outage does not remove live sources.
Before the first successful listing, a failed prefix is skipped for that poll and retried later.
With `on_invalid: warn`, failed additions and replacements also remain pending for the next poll; an unsuccessful replacement keeps serving the last good source.

## Remote COG

COG files can be served from any object store or HTTP(S) endpoint supported by the underlying object-store client, using the same URL schemes and settings as [PMTiles sources](sources-pmtiles.md#serving-pmtiles-from-local-file-systems-http-or-object-storage).
Remote COGs are read with byte-range requests, so Martin fetches the TIFF metadata and image chunks it needs instead of downloading the complete object first.
The shared settings include AWS profiles and runtime task-role discovery, cloud-specific credentials, custom endpoints, proxies, and HTTP client options.
Plain `http://` URLs are enabled by default for COG sources; prefer HTTPS outside trusted networks.

Supported URL schemes include:

- `s3://<bucket>/<prefix>` and `s3a://<bucket>/<prefix>`, also for S3-compatible services such as [MinIO](https://www.min.io/), [Ceph](https://docs.ceph.com/en/latest/radosgw/s3/), [Cloudflare R2](https://developers.cloudflare.com/r2/), and others
- `gs://<bucket>/<prefix>`
- `az://<container>/<prefix>` and the other Azure schemes
- `https://host/path`, `http://host/path`

HTTP(S) URLs can name individual files.
Prefix discovery requires a backend that supports object listing; an ordinary web directory cannot be enumerated.
Only `.tif` and `.tiff` objects under a prefix are published, using the file stem as the initial source ID.

```yaml
cog:
  reload_interval: 1m
  allow_http: true
  paths:
    - s3://my-bucket/imagery/
  sources:
    mosaic: s3://my-bucket/imagery/mosaic.tif
    raster: https://tiles.example.org/mosaic.tif
```

For AWS S3, a directly configured object requires `s3:GetObject`.
Discovering a prefix additionally requires `s3:ListBucket` on the bucket, scoped to that prefix where appropriate.

To connect to a custom S3-compatible endpoint (e.g. MinIO), allow plain `http://` endpoints, or provide credentials, use the object-store options documented in the [PMTiles source documentation](sources-pmtiles.md#serving-pmtiles-from-local-file-systems-http-or-object-storage). For example:

```yaml
cog:
  aws_endpoint: http://localhost:9000
  aws_region: us-east-1
  allow_http: true
  skip_signature: false
  aws_access_key_id: ${AWS_ACCESS_KEY_ID}
  aws_secret_access_key: ${AWS_SECRET_ACCESS_KEY}
  paths:
    - s3://my-bucket/imagery/
```

URL queries are preserved on object requests, so presigned and token-authenticated URLs keep working:

```bash
martin 'https://tiles.example.org/mosaic.tif?token=secret-query'
```

When Martin derives object URLs from a remote prefix, it retains the configured scheme, host, custom port, query, and fragment.
For safety, URL user information, query strings, and fragments are removed from errors, logs, and `--save-config` output.
A saved configuration therefore does not retain a presigned URL token; provide that secret again before restarting from the generated file.

!!! note
    Local files configured directly in `cog.paths` or `cog.sources` are loaded at startup but are not watched for changes.
    Remote objects in either setting are polled, while remote prefixes in `cog.paths` are re-listed.

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

# gdalwarp
gdalwarp src1.tif src2.tif out.tif -of COG

# or gdal_translate
gdal_translate input.tif output_cog.tif -of COG
```

### The mapping from ZXY to tiff chunk

- A single TIFF file could contains many sub-file about same spatial area, each has different resolution
- A sub file is organized with many tiles

So basically there's a mapping from zxy to tile of sub-file of TIFF.

| zxy        | mapping to                  |
| ---------- | --------------------------- |
| Zoom level | which sub-file in TIFF file |
| X and Y    | which tile in subfile       |

Clients could read only the header part of COG to figure out the mapping from zxy to the chunk number and the subfile number.
Martin get tile to frontend by this mapping.
