## MBTiles and PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files. To serve a file from CLI, simply put the path to the file or
the directory with `*.mbtiles` or `*.pmtiles` files. A path to PMTiles file may be a URL. For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

## PMTiles S3

Martin supports authenticated S3 sources using environment variables.

By default, the credentials try for default profile or environment variables. Environment variables that can to be set:

- `AWS_ACCESS_KEY_ID`
- `AWS_SECRET_ACCESS_KEY`
- `AWS_SESSION_TOKEN`
- `AWS_PROFILE` (to specify profile instead of access key variables)
- `AWS_REGION`

Note: `AWS_REGION` must match the region of the given bucket in the S3 URI.

For more information on AWS credentials, see [documentation](https://docs.aws.amazon.com/sdkref/latest/guide/creds-config-files.html).

### Anonymous credentials

To send requests anonymously for publicly available buckets, set the environment variable `AWS_NO_CREDENTIALS=1`.
Note that you still need to set `AWS_REGION` to the correct region.

Example configuration:

```yaml
pmtiles:
  sources:
    tiles: s3://bucket/path/to/tiles.pmtiles
```
