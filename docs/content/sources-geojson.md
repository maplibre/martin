---
icon: material/vector-polygon
tags:
  - geojson
  - tile-sources
  - configuration
---

# GeoJSON Feature Serving

A simple way to add geospatial data to a map can be to serve the content of [GeoJSON files](https://geojson.org/) as specified by [RFC7946](https://datatracker.ietf.org/doc/html/rfc7946).
Instead of incurring the overhead of serving them directly, we serve them as Vector tiles.

To serve a file from CLI, simply put the path to the file or the directory with `*.geojson` files.
For example:

```bash
martin /path/to/geojson/file.geojson /path/to/directory
```

You may also want to generate a [config file](config-file/index.md) using `--save-config my-config.yaml`.
The config file can then be used via the `--config my-config.yaml` option.

!!! warning
    Serving these files is less efficient compared to pre-calculated [PMTiles or MBTiles](sources-files.md).
    This is because to serve GeoJSON, martin needs to:

    - parse JSON
    - reproject geometrys
    - clip geometry to the requested tiles
    - encode them as MVT

    To improve performance, we currently assume that each file fits into RAM.

## Run Martin with configuration file

```yaml
geojson:
  # Optionally override the MVT tile grid extent for every GeoJSON source. Defaults to 4096.
  extent: 4096
  # Optionally override the clip margin kept around each tile edge, in tile units. Defaults to 64.
  buffer: 64
  paths:
    # Scan this whole directory, publishing every *.json/*.geojson file as a source.
    - /path/to/geojson/directory
    # A specific file is published as a single source.
    - /path/to/file.geojson
  sources:
    # A named source mapping a source name to a single file.
    foo: /path/to/file.geojson
```

The type-level `extent` and `buffer` apply to every GeoJSON source:

- **`extent`** - side length of the MVT tile coordinate grid each tile is encoded into (defaults to `4096`, the value [MapLibre](https://maplibre.org/) assumes). Must be non-zero.
- **`buffer`** - clip margin kept around each tile edge, in tile units (defaults to `64`). Increase it if you see seam artifacts on line caps/joins or polygon outlines near tile edges.

## GeoJSON Hot Reload

Martin watches directories configured under `geojson` for `.json`/`.geojson` changes at runtime.
When files are added, modified, or removed from a watched directory, Martin automatically updates the tile catalog - no restart required.

```bash
# Martin will watch this directory and reflect any *.geojson changes live
martin /path/to/geojson/directory
```

Or via config file:

```yaml
geojson:
  paths:
    - /path/to/geojson/directory
  sources:
    foo: /path/to/file.geojson
```

The following events are handled automatically:

- **File added** - the new source appears in the catalog.
- **File modified** - the source is reloaded and its tile cache is invalidated.
- **File removed** - the source is removed from the catalog.

!!! note
    Hot reload applies to directories configured under `geojson.paths` (or passed on the CLI).
    Named sources listed under `geojson.sources` are snapshotted at startup and are not watched for changes.

If you want to convert your geojson files to [mbtiles/ pmtiles](sources-files.md), we recomend tooling like [`tippecanoe`](https://github.com/felt/tippecanoe).
