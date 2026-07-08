---
icon: material/database
tags:
  - mbtiles
  - pmtiles
  - tile-sources
  - configuration
---

# MBTiles and PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files.
To serve a file from CLI, simply put the path to the file or the directory with `*.mbtiles` or `*.pmtiles` files.
A path to PMTiles file may be a URL.
For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](../config-file/index.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

!!! tip
    See [our tile sources explanation](../sources-tiles/index.md) for a more detailed explanation on the difference between our available data sources.

Both formats have their own dedicated page describing how to configure them:

- [MBTiles File Sources](../sources-mbtiles.md) - local SQLite archives.
- [PMTiles File Sources](../sources-pmtiles.md) - local or remote (HTTP range / object storage) archives.

## MBTiles vs PMTiles

MBTiles and PMTiles are both formats for storing tiled geospatial data, but they differ in architecture, deployment, and operational characteristics.

### Key Differences

- **Deployment model**
  **MBTiles** archives must be stored locally on the same machine as the tile server.
  **PMTiles** archives can be accessed either locally or remotely via HTTP range requests, e.g. from an object storage like S3.
  **PMTiles** allows simpler production deployment with Kubernetes, as it allows the large data file to reside in S3 and shared by multiple pods, but restricted from direct user access.
- **Performance**
  **MBTiles** typically provide slightly lower latency due to local access and SQLite indexing.
  **PMTiles** may introduce additional latency when accessed remotely, but this is usually mitigated by HTTP caching and CDN usage.
- **Storage efficiency**
  **PMTiles** archives are generally more space-efficient, typically ~10-15% smaller than equivalent **MBTiles** archives.
- **Memory usage**
  **MBTiles** relies on SQLite, which maintains an internal page cache and may increase memory usage under load.
  **PMTiles** can, in some cases, operate with lower memory overhead, depending on access patterns and caching configuration.

The choice between MBTiles and PMTiles depends on system requirements:

- Use **MBTiles** for local, self-contained deployments with minimal external dependencies.
- Use **PMTiles** for cloud-native or distributed setups where remote access, CDN integration, or object storage is preferred.

## Postprocessing

MBTiles and PMTiles sources support `convert_to_mlt` and `convert_to_mvt` keys to control tile postprocessing.
This can be set for all sources of a type or for an individual source.
See [Configuration File](../config-file/index.md#postprocessing) for details.

```yaml
pmtiles:
  sources:
    basemap:
      path: /data/basemap.pmtiles
      convert_to_mlt: auto    # convert MVT -> MLT when client requests it
      convert_to_mvt: auto    # convert MLT -> MVT when client requests it
    mlt_archive:
      path: /data/mlt_tiles.pmtiles
      convert_to_mvt: disable # disabllow any on the the fly conversion
      convert_to_mlt: disable
```

## Autodiscovery

For mbtiles or local pmtiles files, we support auto discovering at startup.
This means that the following will discover all mbtiles and pmtiles files in the directory:

```bash
martin  /path/to/directory
```

For remote PMTiles, individual file URLs work as expected.
Remote object-storage *prefixes* (e.g. `s3://bucket/tiles/`) are also supported via periodic listing - see [PMTiles Hot Reload](../sources-pmtiles.md#pmtiles-hot-reload).
