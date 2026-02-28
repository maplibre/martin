# Generating Tiles in Bulk

We offer the `martin-cp` tool for generating tiles in bulk, from any source(s) supported by Martin, and save retrieved tiles into a new or an existing MBTiles file.

`martin-cp` can be used to generate tiles for a large area or multiple areas (bounding boxes).
If multiple areas overlap, it will ensure each tile is generated only once
`martin-cp` supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

After copying, `martin-cp` will update the `agg_tiles_hash` metadata value unless `--skip-agg-tiles-hash` is specified.
This allows the MBTiles file to be [validated](mbtiles-validation.md#aggregate-content-validation) using `mbtiles validate` command.

## Usage

This copies tiles from a PostGIS table `my_table` into an MBTiles file `tileset.mbtiles` using [normalized](mbtiles-schema.md#normalized) schema, with zoom levels from 0 to 10, and xyz-compliant tile bounds of the whole world.

```bash
martin-cp  --output-file tileset.mbtiles                         \
           --mbtiles-type normalized                             \
           "--bbox=-180,-85.05112877980659,180,85.0511287798066" \
           --min-zoom 0                                          \
           --max-zoom 10                                         \
           --source source_name                                  \
           postgres://postgres@localhost:5432/db
```

!!! tip
    > Next to regular sorces, `--source <SOURCE>` does support [composite sources](sources-composite.md).
    > This means `martin-cp` can be used to merge two different sources into one `mbtiles` archive.

If performance is a concern, you should also consider

!!! tip
    > `--concurrency <CONCURRENCY>` and `--pool-size <POOL_SIZE>` can be used to control the number of concurrent requests and the pool size for postgres sources respectively.
    >
    > The optimal setting depends on:
    >
    > - the source(s) performance characteristics
    > - how much load is allowed, for example in a multi-tenant environment
    > - how to compress tiles stored in the output file

You should also consider

!!! tip
    > `--encoding <ENCODING>` can be used to reduce the final size of the MBTiles file or decrease the amount of processing `martin-cp` does.
    >
    > The default `gzip` should be a reasonable choice for most use cases, but if you prefer a different encoding, you can specify it here.
    > If set to multiple values like `'gzip,br'`, `martin-cp` will use the first encoding, or re-encode if the tile is already encoded and that encoding is not listed.
    > Use `identity` to disable compression.
    > Ignored for non-encodable tiles like PNG and JPEG.

## Arguments

Use `martin-cp --help` to see a list of available options:

```text
--8<-- "help/martin-cp.txt"
```
