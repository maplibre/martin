## MBTiles and PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files. To serve a file from CLI, simply put the path to the file or
the directory with `*.mbtiles` or `*.pmtiles` files. A path to PMTiles file may be a URL. For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

> [!TIP]
> See [our tile sources explanation](sources-tiles.md) for a more detailed explanation on the difference between our available data sources.
>
> The difference between MBTiles and PMTiles is that:
>
> - **MBTiles** require the entire archive to be on the same machine. **PMTiles** can utilise a remote HTTP-Range request supporting server or a local file.
> - Performance wise, **MBTiles** is slightly faster than **PMTiles**, but with caching this is negligible.
> - Disk size wise, **MBTiles** is slightly (10-15%) higher than **PMTiles**.
> - **PMTiles** requires less memory in extreme cases as sqlite has a small in-memory cache.
>
> The choice depends on your specific usecase and requirements.
### Autodiscovery

For mbtiles or local pmtiles files, we support auto discovering at startup.
This means that the following will discover all mbtiles and pmtiles files in the directory:

```bash
martin  /path/to/directory
```

> [!WARNING]
> For remote PMTiles, we don't currently support auto-discovery.
> If you want to implement this feature, please see <https://github.com/maplibre/martin/issues/2180>
>
> We also don't currently support refreshing the catalog at runtime.
> If you want to implement this feature, please see <https://github.com/maplibre/martin/issues/288> instead.

### Serving PMTiles from local file systems, http or Object Storage

The settings avaliable for a PMTiles source depend on the backend:

{{#tabs }}
{{#tab name="local file system" }}

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

{{#endtab }}
{{#tab name="Http(s)" }}

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

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Amazon S3" }}

> [!IMPORTANT]
> Even though we name this section `Amazon S3`, it also works with other providers that support the S3 API, such as [MinIO](https://www.min.io/), [Ceph](https://docs.ceph.com/en/latest/radosgw/s3/), [Cloudflare R2](https://developers.cloudflare.com/r2/), [hetzner object storage](https://www.hetzner.com/de/storage/object-storage/) and many more.

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

> [!TIP]
> All settings are also available under the `aws_` prefix.
> This can be useful if you want to have different cloud providers.

### Available AWS S3 settings

{{#include pmtiles/aws.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Google Cloud Storage" }}

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

> [!TIP]
> All settings are also available under the `google_` prefix.
> This can be useful if you want to have different cloud providers.

### Available google settings

{{#include pmtiles/google.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Microsoft Azure" }}

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

> [!TIP]
> All settings are also available under the `azure_` prefix.
> This can be useful if you want to have different cloud providers.

### Available azure settings

{{#include pmtiles/azure.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#endtabs}}
