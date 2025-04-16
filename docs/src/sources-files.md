## MBTiles and PMTiles File Sources

Martin can serve any type of tiles from [PMTile](https://protomaps.com/blog/pmtiles-v3-whats-new)
and [MBTile](https://github.com/mapbox/mbtiles-spec) files. To serve a file from CLI, simply put the path to the file or
the directory with `*.mbtiles` or `*.pmtiles` files. A path to PMTiles file may be a URL. For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory   https://example.org/path/tiles.pmtiles
```

You may also want to generate a [config file](config-file.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.
