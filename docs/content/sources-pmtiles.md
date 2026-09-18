---
icon: material/database
tags:
  - pmtiles
  - tile-sources
  - configuration
  - aws
  - azure
  - google-cloud
---

# PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new) files.
A PMTiles archive can be accessed either locally or remotely via HTTP range requests, e.g. from an object storage like S3.
A path to a PMTiles file may be a URL.
For example:

```bash
martin  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file/index.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

!!! tip
    See [MBTiles vs PMTiles](sources-files/index.md#mbtiles-vs-pmtiles) for a comparison of the two file formats.

## PMTiles Hot Reload

Martin watches local directories configured under `pmtiles` for `.pmtiles` files using filesystem events, with the same add/modify/remove semantics described for [MBTiles](sources-mbtiles.md#mbtiles-hot-reload).

```yaml
pmtiles:
  paths:
    - /path/to/pmtiles/directory
```

!!! tip "Scanning subdirectories"
    Set `recursive: true` next to `paths` to scan subdirectories too.
    A nested file is named by its path relative to the scanned directory with `/` replaced by `.`, so `2024/roads.pmtiles` becomes `2024.roads`.

!!! tip "Per-project directories"
    List a directory of project directories under `collections` to publish every file inside a project as `<project>.<file>`, so `/projects/tiles/project1/roads.pmtiles` becomes `project1.roads`.
    A collection is a local directory.

For remote object-storage prefixes (`s3://bucket/prefix/`, `gs://bucket/prefix/`, etc.) Martin periodically re-lists the prefix and diffs against the previous snapshot, taking into account
object `ETag` or `Last-Modified` headers to detect updates to an existing source.
There is no event channel from blob storage to subscribe to.
Added, updated, and removed objects propagate to the catalog.

```yaml
pmtiles:
  paths:
    - s3://my-bucket/tiles/
  reload_interval: 10m  # default; set to "0s" to disable remote polling
```

!!! note
    Hot reload applies to directories and remote prefixes configured under `pmtiles.paths` (or passed on the CLI).
    Named sources listed under `pmtiles.sources` and individual remote-file URLs are snapshotted at startup and are not watched for changes.

## Serving PMTiles without a Tile Server

PMTiles archives can be served directly from HTTP range-capable storage without a dedicated tile server.
This approach has several limitations:

- **Unrestricted access risk**
  Without proper access controls, clients may download large portions (or all) of an archive, leading to significant egress costs.
  A tile server restricts access to tile requests, but bulk extraction remains possible via many requests, which are generally easier to detect and block.
- **Over-fetching**
  PMTiles may fetch more data than strictly required per tile request to minimize the number of HTTP requests.
- **Lack of source composition**
  Direct serving does not support combining PMTiles with dynamic data sources (e.g., PostGIS) into a unified tile service.
  A tile server (e.g, Martin) is required for this.
- **Caching behavior**
  Cache efficiency may be reduced compared to setups with a dedicated tile server that can optimize request patterns.

## Serving PMTiles from local file systems, HTTP, or object storage

### Local files

Pass a path or `file://` URL on the command line:

```bash
martin path/to/tiles.pmtiles
```

Or configure a named source:

```yaml
pmtiles:
  sources:
    tiles: file:///path/to/tiles.pmtiles
```

### Remote files and prefixes

PMTiles supports HTTP(S), Amazon S3 and compatible services, Google Cloud Storage, and Microsoft Azure Storage.
See [Remote Object Storage](sources-object-storage.md) for URL schemes, credentials, cloud profiles, custom endpoints, proxy settings, and HTTP client configuration.

```yaml
pmtiles:
  sources:
    tiles: s3://my-bucket/tiles.pmtiles
```
