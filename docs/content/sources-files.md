# MBTiles and PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files. To serve a file from CLI, simply put the path to the file or
the directory with `*.mbtiles` or `*.pmtiles` files. A path to PMTiles file may be a URL. For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

!!! tip
    See [our tile sources explanation](sources-tiles.md) for a more detailed explanation on the difference between our available data sources.

### Postprocessing

MBTiles and PMTiles sources support a `process` block to control tile postprocessing (MLT conversion, compression).
This can be set for all sources of a type or for an individual source.
See [Configuration File](config-file/index.md#postprocessing) for details.

```yaml
pmtiles:
  process: null
  sources:
    basemap:
      path: /data/basemap.pmtiles
      process:
        mlt: auto
```

### Autodiscovery

For mbtiles or local pmtiles files, we support auto discovering at startup.
This means that the following will discover all mbtiles and pmtiles files in the directory:

```bash
martin  /path/to/directory
```

!!! warning
    For remote PMTiles, we don't currently support auto-discovery.
    If you want to implement this feature, please see <https://github.com/maplibre/martin/issues/2180>

### MBTiles Hot Reload

Martin watches directories configured under `mbtiles` for changes at runtime. When `.mbtiles` files are added, modified, or removed from a watched directory, Martin automatically updates the tile catalog — no restart required.

```bash
# Martin will watch this directory and reflect any *.mbtiles changes live
martin  /path/to/mbtiles/directory
```

Or via config file:

```yaml
mbtiles:
  paths:
    - /path/to/mbtiles/directory
```

The following events are handled automatically:

- **File added** - the new source appears in the catalog.
- **File modified** - the source is reloaded and its tile cache is invalidated.
  Not avaliable on windows due to OS-limtations (SQLite not allowing `FILE_SHARE_DELETE`).
- **File removed** - the source is removed from the catalog.

!!! note
    Hot reload applies to directories configured under `mbtiles.paths` (or passed on the CLI). Named sources listed under `mbtiles.sources` are snapshotted at startup and are not watched for changes.

!!! note
    PMTiles hot reload is not yet supported. If you want to help implement it, see <https://github.com/maplibre/martin/issues/2180>.

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
  **PMTiles** archives are generally more space-efficient, typically ~10–15% smaller than equivalent **MBTiles** archives.
- **Memory usage**
  **MBTiles** relies on SQLite, which maintains an internal page cache and may increase memory usage under load.
  **PMTiles** can, in some cases, operate with lower memory overhead, depending on access patterns and caching configuration.

### Serving PMTiles without a Tile Server

PMTiles archives can be served directly from HTTP range–capable storage without a dedicated tile server. This approach has several limitations:

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

The choice between MBTiles and PMTiles depends on system requirements:

- Use **MBTiles** for local, self-contained deployments with minimal external dependencies.
- Use **PMTiles** for cloud-native or distributed setups where remote access, CDN integration, or object storage is preferred.

### Serving PMTiles from local file systems, http or Object Storage

The settings available for a PMTiles source depend on the backend:

=== "Local File System"

    For local sources, you need to provide the path or URL.
    For example:

    ```bash
    martin  path/to/tiles.pmtiles
    ```

    The available schemes are:

    - `file:///path/to/my/file.pmtiles`
    - `path/to/my/file.pmtiles`

    You can also configure this via the configuration file:

    ```yaml
    pmtiles:
      sources:
        tiles: file:///path/to/my/file.pmtiles
    ```

=== "Http(s)"

    For HTTP(s), you need to provide the url.
    For example:

    ```bash
    martin  https://example.com/tiles.pmtiles
    ```

    The available url schemes are:

    - `http://example.com/path.pmtiles`
    - `https://example.com/path.pmtiles`

    If you want more control over your requests, you can configure additional options here as such:

    ```yaml
    pmtiles:
      allow_http: true
      sources:
        tiles: s3://bucket/path/to/tiles.pmtiles
    ```

    ### Available http client settings

    --8<-- "pmtiles/client.md"

=== "Amazon S3"

    !!! info "Important"
        Even though we name this section `Amazon S3`, it also works with other providers that support the S3 API, such as [MinIO](https://www.min.io/), [Ceph](https://docs.ceph.com/en/latest/radosgw/s3/), [Cloudflare R2](https://developers.cloudflare.com/r2/), [hetzner object storage](https://www.hetzner.com/de/storage/object-storage/) and many more.

    For AWS, you need to provide the bucket name and the prefix of the object key.
    For example:

    ```bash
    martin  s3://my-bucket/tiles.pmtiles
    ```

    The available url schemes are:

    - `s3://<bucket>/<path>`
    - `s3a://<bucket>/<path>`
    - `https://s3.<region>.amazonaws.com/<bucket>`
    - `https://<bucket>.s3.<region>.amazonaws.com`
    - `https://ACCOUNT_ID.r2.cloudflarestorage.com/bucket`

    If you want more control over your requests, you can configure additional options here as such:

    ```yaml
    pmtiles:
      allow_http: true
      sources:
        tiles: s3://bucket/path/to/tiles.pmtiles
    ```

    !!! tip
        All settings are also available under the `aws_` prefix.
        This can be useful if you want to have different cloud providers.

    ### Available AWS S3 settings

    --8<-- "pmtiles/aws.md"

    --8<-- "pmtiles/client.md"

=== "Google Cloud Storage"

    For Google Cloud, you need to provide the bucket name and the prefix of the object key.
    For example:

    ```bash
    martin  gs://my-bucket/tiles.pmtiles
    ```

    The available url scheme is:

    - `gs://bucket/path`

    If you want more control over your requests, you can configure additional options here as such:

    ```yaml
    pmtiles:
      allow_http: true
      sources:
        tiles: gs://bucket/path/to/tiles.pmtiles
    ```

    !!! tip
        All settings are also available under the `google_` prefix.
        This can be useful if you want to have different cloud providers.

    ### Available google settings

    --8<-- "pmtiles/google.md"

    --8<-- "pmtiles/client.md"

=== "Microsoft Azure"

    For Azure, you need to provide the account name, container and path.
    For example:

    ```bash
    martin  az://my-container/tiles.pmtiles
    ```

    The available url schemes are:

    - `abfs[s]://<container>/<path>` (according to [fsspec](https://github.com/fsspec/adlfs))
    - `abfs[s]://<file_system>@<account_name>.dfs.core.windows.net/<path>`
    - `abfs[s]://<file_system>@<account_name>.dfs.fabric.microsoft.com/<path>`
    - `az://<container>/<path>` (according to [fsspec](https://github.com/fsspec/adlfs))
    - `adl://<container>/<path>` (according to [fsspec](https://github.com/fsspec/adlfs))
    - `azure://<container>/<path>` (custom)
    - `https://<account>.dfs.core.windows.net`
    - `https://<account>.blob.core.windows.net`
    - `https://<account>.blob.core.windows.net/<container>`
    - `https://<account>.dfs.fabric.microsoft.com`
    - `https://<account>.dfs.fabric.microsoft.com/<container>`
    - `https://<account>.blob.fabric.microsoft.com`
    - `https://<account>.blob.fabric.microsoft.com/<container>`

    If you want more control over your requests, you can configure additional options here as such:

    ```yaml
    pmtiles:
      allow_http: true
      sources:
        tiles: az://my-container/tiles.pmtiles
    ```

    !!! tip
        All settings are also available under the `azure_` prefix.
        This can be useful if you want to have different cloud providers.

    ### Available azure settings

    --8<-- "pmtiles/azure.md"

    --8<-- "pmtiles/client.md"
