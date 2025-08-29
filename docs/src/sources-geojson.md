## GeoJSON Feature Serving

A simple way to add geospatial data to a map can be to serve the content of [GeoJSON files](https://geojson.org/) as specified by [RFC7946](https://datatracker.ietf.org/doc/html/rfc7946).
Instead of incurring the overhead of serving them directly, we serve them as Vector tiles.

To serve a file from CLI, simply put the path to the file or the directory with `*.geojson` files.
For example:

```bash
martin /path/to/geojson/file.geojson /path/to/directory
```

You may also want to generate a [config file](config-file.md) using `--save-config my-config.yaml`.
The config file can then be used via the `--config my-config.yaml` option.

> [!WARNING]
> Serving these files is less efficient compared to [PMTiles or MBTiles](sources-files.md).
> This is because to serve GeoJSON, martin needs to:
> 
> - parse JSON
> - reproject geometrys
> - clip geometry to the requested tiles
> - encode them as MVT
>
> To improve performance, we currently assume that the data on disk is static and fits into RAM.
>
> Us loading data on startup also means that updates to the JSON files or new files are currently not propergated and require a restart.
> Please see [#288](https://github.com/maplibre/martin/issues/288) for further context.

If you want to convert your geojson files to [mbtiles/ pmtiles](sources-files.md), we recomend tooling like [`tippecanoe`](https://github.com/felt/tippecanoe).