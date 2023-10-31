# CLI Tools 

Martin project contains additional tooling to help manage the data servable with Martin tile server.

## `mbtiles`
`mbtiles` is a small utility to interact with the `*.mbtiles` files from the command line. It allows users to examine, copy, validate, compare, and apply diffs between them.

Use `mbtiles --help` to see a list of available commands, and `mbtiles <command> --help` to see help for a specific command.

This tool can be installed by compiling the latest released version with `cargo install mbtiles`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

The `mbtiles` utility builds on top of the [MBTiles specification](https://github.com/mapbox/mbtiles-spec). It adds a few additional conventions to ensure that the content of the tile data is valid, and can be used for reliable diffing and patching of the tilesets. 
