# Provided Tools

In addition to the `martin` tile server, we provide a set of tools to help build and manage maps. These tools are designed to work seamlessly with the `martin` server and can be used to generate tiles, manage data, and perform various operations on maps.

## CLI Tools

Martin project contains additional tooling to help manage the data servable with Martin tile server.

### `martin cp`

`martin cp` is a subcommand of `martin` for copying tiles from any sources into a new or an existing tile archive. It supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources. It can copy tiles in two ways:

- **One by one**: `martin cp` asks the source for each tile, so the source must either contain tiles (MBTiles, PMTiles) or generate them (PostgreSQL's `ST_AsMVT`). It can be used for a large area or multiple areas, and if multiple areas overlap, it will copy each tile only once. See [this article](<https://maplibre.org/martin/martin-cp/index.md>) for more information.
- **In bulk**: `martin cp` reads each source's features once and builds the whole tileset inside Martin. It uses the tile generation method and a similar layer configuration adapted from [Planetiler](<https://github.com/onthegomap/planetiler>) by Mike Barry. This feature is unstable. See [this article](<https://maplibre.org/martin/martin-generate/index.md>) for more information.

### `mbtiles`

`mbtiles` is a small utility to interact with the `*.mbtiles` files from the command line. It allows users to [examine](<https://maplibre.org/martin/mbtiles-meta/index.md>), [copy](<https://maplibre.org/martin/mbtiles-copy/index.md>), [validate](<https://maplibre.org/martin/mbtiles-validation/index.md>) or [compare and apply diffs between them](<https://maplibre.org/martin/mbtiles-diff/index.md>).

See [this article](<https://maplibre.org/martin/mbtiles/index.md>) for more information.

## Supporting crates

Next to these tools, we also have a set of supporting crates for supporting the `martin` server and its ecosystem. Example of this is `martin-tile-utils` which is used in `martin` and `mbtiles`.
