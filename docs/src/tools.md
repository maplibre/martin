# Provided Tools

Next to the `martin` server, we provide a set of tools to help build and manage maps. These tools are designed to work seamlessly with the `martin` server and can be used to generate tiles, manage data, and perform various operations on maps.

## CLI Tools

Martin project contains additional tooling to help manage the data servable with Martin tile server.

### `martin-cp`

`martin-cp` is a tool for generating tiles in bulk, and save retrieved tiles into a new or an existing MBTiles file. It can be used to generate tiles for a large area or multiple areas.
If multiple areas overlap, it will generate tiles only once.
`martin-cp` supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

See [this article](martin-cp.md) for more information.

### `mbtiles`

`mbtiles` is a small utility to interact with the `*.mbtiles` files from the command line.
It allows users to [examine](mbtiles-meta.md), [copy](mbtiles-copy.md), [validate](mbtiles-validation.md) or [compare and apply diffs between them](mbtiles-diff.md).

See [this article](mbtiles.md) for more information.

### Supporting crates

Next to these tools, we also have a set of supporting crates for supporting the `martin` server and its ecosystem.
Example of this is `martin-tile-utils` which is used in `martin`, `mbtiles` and `martin-cp`.
