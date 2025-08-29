## MBTiles and PMTiles Tile Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files.
To serve a file from CLI, simply put the path to the file or the directory with `*.mbtiles` or `*.pmtiles` files.
A path to PMTiles file may be a URL.
For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file.md) using `--save-config my-config.yaml`.
The config file can then be used via the `--config my-config.yaml` option.

> [!IMPORTANT]
> The difference between MBTiles and PMTiles is that:
> 
> - **MBTiles** require the entire archive to be on the same machine. **PMTiles** can utilise a remote HTTP-Range request supporting server or a local file.
> - Performance wise, **MBTiles** is slightly faster than **PMTiles**, but with caching this is negligible.
> - Disk size wise, **MBTiles** is slightly (10-15%) higher than **PMTiles**.
> - **PMTiles** requires less memory in extreme cases as sqlite has a small in-memory cache.
> 
> The choice depends on your specific usecase and requirements.

### Serving PMTiles via S3

#### Authentication with AWS credentials

Martin supports authenticated S3 sources using environment variables.

By default, Martin will use default profile's credentials unless these [AWS environment variables](https://docs.aws.amazon.com/sdkref/latest/guide/creds-config-files.html) are set:

- `AWS_ACCESS_KEY_ID`
- `AWS_SECRET_ACCESS_KEY`
- `AWS_SESSION_TOKEN`
- `AWS_PROFILE` - to specify profile instead of access key variables
- `AWS_REGION` - if set, must match the region of the bucket in the S3 URI

#### Anonymous credentials

By default, martin does require credentials for S3 buckets.
To send requests anonymously for publicly available buckets, set the environment variable `AWS_SKIP_CREDENTIALS=1` or configuration key `skip_credentials: true` respectively.

Note: you still need to set `AWS_REGION` to the correct region.

Example configuration:

```yaml
pmtiles:
  skip_credentials: false
  sources:
    tiles: s3://bucket/path/to/tiles.pmtiles
```

#### Url styles

We also support forcing path style URLs for S3 buckets via the environment variable `AWS_S3_FORCE_PATH_STYLE=1` or configuration key `force_path_style: true`.
This allows you to use this functionality for [`MinIO`](https://min.io/) or similar s3-compatible instances which use path style URLs.
A path style URL is a URL that uses the bucket name as part of the path (`example.org/some_bucket`) instead of the hostname (`some_bucket.example.org`).

Example configuration:

```yaml
pmtiles:
  force_path_style: true
  sources:
    tiles: s3://bucket/path/to/tiles.pmtiles
```
