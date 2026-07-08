---
icon: material/database
tags:
  - mbtiles
  - tile-sources
  - configuration
---

# MBTiles File Sources

Martin can serve any type of tiles from [MBTile](https://github.com/mapbox/mbtiles-spec) files.
MBTiles archives are local SQLite databases and must reside on the same machine as the tile server.
To serve a file from CLI, simply put the path to the file or the directory with `*.mbtiles` files.
For example:

```bash
martin  /path/to/mbtiles/file.mbtiles  /path/to/directory
```

You may also want to generate a [config file](config-file/index.md) using the `--save-config my-config.yaml`, and later edit
it and use it with `--config my-config.yaml` option.

!!! tip
    See [MBTiles vs PMTiles](sources-files/index.md#mbtiles-vs-pmtiles) for a comparison of the two file formats.

## MBTiles Hot Reload

Martin watches directories configured under `mbtiles` for changes at runtime.
When `.mbtiles` files are added, modified, or removed from a watched directory, Martin automatically updates the tile catalog - no restart required.

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
