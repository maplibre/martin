# Generating Tiles in Bulk

`martin-cp` is a tool for generating tiles in bulk, and save retrieved tiles into a new or an existing MBTiles file. It can be used to generate tiles for a large area or multiple areas. If multiple areas overlap, it will generate tiles only once. `martin-cp` supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

## Usage

This copies tiles from a PostGIS table `my_table` into an MBTiles file `tileset.mbtiles` using [normalized](mbtiles-schema.md) schema, with zoom levels from 0 to 10, and bounds of the whole world. 

```shell
martin-cp  --output-file tileset.mbtiles \
           --mbtiles-type normalized     \
           "--bbox=-180,-90,180,90"      \
           --min-zoom 0                  \
           --max-zoom 10                 \
           --source my_table             \
           postgresql://postgres@localhost:5432/db
```
