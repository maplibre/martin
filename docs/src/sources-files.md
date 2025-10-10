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

### Serving PMTiles from Object Storage

Next to local files and remote HTTP sources, we support serving PMTiles from object storage.
All major cloud providers, including AWS S3, Google Cloud Storage, and Azure Blob Storage are supported.
The providers differ in the options they support.

To serve PMTiles from a provider, you need to provide the bucket name and the prefix of the object key.
For example:

```bash
martin  s3://my-bucket/tiles.pmtiles
```

The available url schemes are:

{{#tabs }}
{{#tab name="local file system" }}

- `file:///path/to/my/file`
- `path/to/my/file`

{{#endtab }}
{{#tab name="Http(s)" }}

- `http://mydomain/path`
- `https://mydomain/path`

{{#endtab }}
{{#tab name="Amazon S3" }}

- `s3://bucket/path`
- `s3a://bucket/path`

{{#endtab }}
{{#tab name="Google Cloud Storage" }}

- `gs://bucket/path`

{{#endtab }}
{{#tab name="Microsoft Azure" }}

- `az://account/container/path`
- `adl://account/container/path`
- `azure://account/container/path`
- `abfs://account/container/path`
- `abfss://account/container/path`

{{#endtab }}
{{#endtabs}}

If you want more control over your requests, you can configure additional options here as such:

```yaml
pmtiles:
  allow_http: true
  sources:
    tiles: s3://bucket/path/to/tiles.pmtiles
```

The available options depend on the underlying source:

{{#tabs }}
{{#tab name="local file system" }}
Files don't currently support additional options.
{{#endtab }}
{{#tab name="Http(s)" }}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Amazon S3" }}

> [!TIP]
> All settings are also available under the `aws_` prefix.
> This can be useful if you want to have different cloud providers.

{{#include pmtiles/aws.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Google Cloud Storage" }}

> [!TIP]
> All settings are also available under the `google_` prefix.
> This can be useful if you want to have different cloud providers.

{{#include pmtiles/aws.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#tab name="Microsoft Azure" }}

> [!TIP]
> All settings are also available under the `azure_` prefix.
> This can be useful if you want to have different cloud providers.

{{#include pmtiles/azure.md}}

{{#include pmtiles/client.md}}

{{#endtab }}
{{#endtabs }}
