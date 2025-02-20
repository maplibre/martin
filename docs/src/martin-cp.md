# Generating Tiles in Bulk

`martin-cp` is a tool for generating tiles in bulk, from any source(s) supported by Martin, and save retrieved tiles
into a new or an existing MBTiles file. `martin-cp` can be used to generate tiles for a large area or multiple areas
(bounding boxes). If multiple areas overlap, it will ensure each tile is generated only once. `martin-cp` supports the
same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

After copying, `martin-cp` will update the `agg_tiles_hash` metadata value unless `--skip-agg-tiles-hash` is specified.
This allows the MBTiles file to be [validated](./mbtiles-validation.md#aggregate-content-validation)
using `mbtiles validate` command.

## Usage

This copies tiles from a PostGIS table `my_table` into an MBTiles file `tileset.mbtiles`
using [normalized](mbtiles-schema.md) schema, with zoom levels from 0 to 10, and bounds of the whole world.

```bash
martin-cp  --output-file tileset.mbtiles \
           --mbtiles-type normalized     \
           "--bbox=-180,-90,180,90"      \
           --min-zoom 0                  \
           --max-zoom 10                 \
           --source source_name          \
           postgresql://postgres@localhost:5432/db
```
