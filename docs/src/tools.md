# CLI Tools

Martin project contains additional tooling to help manage the data servable with Martin tile server.

## `martin-cp`

`martin-cp` is a tool for generating tiles in bulk, and save retrieved tiles into a new or an existing MBTiles file. It can be used to generate tiles for a large area or multiple areas. If multiple areas overlap, it will generate tiles only once. `martin-cp` supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

## `mbtiles`

`mbtiles` is a small utility to interact with the `*.mbtiles` files from the command line. It allows users to examine, copy, validate, compare, and apply diffs between them.

Use `mbtiles --help` to see a list of available commands, and `mbtiles <command> --help` to see help for a specific command.

This tool can be installed by compiling the latest released version with `cargo install mbtiles --locked`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

The `mbtiles` utility builds on top of the [MBTiles specification](https://github.com/mapbox/mbtiles-spec). It adds a few additional conventions to ensure that the content of the tile data is valid, and can be used for reliable diffing and patching of the tilesets.
