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

For remote object-storage prefixes (`s3://bucket/prefix/`, `gs://bucket/prefix/`, `https://host/prefix/`, etc.) Martin periodically re-lists the prefix and diffs against the previous snapshot, taking into account
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

## Serving PMTiles from local file systems, http or Object Storage

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
