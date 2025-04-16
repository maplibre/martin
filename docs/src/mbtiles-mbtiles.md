# Working with MBTiles archives

We offer the `mbtiles` utility to interact with the `*.mbtiles` files (see [this page](mbtiles-schema.md) for more information) from the command line.
It allows users to [examine](mbtiles-meta.md), [copy](mbtiles-copy.md), [validate](mbtiles-validation.md) or [compare and apply diffs between them](mbtiles-diff.md).

Use `mbtiles --help` to see a list of available commands, and `mbtiles <command> --help` to see help for a specific command.

This tool can be installed by compiling the latest released version with `cargo install mbtiles --locked`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

The `mbtiles` utility builds on top of the [MBTiles specification](https://github.com/mapbox/mbtiles-spec). It adds a few additional conventions to ensure that the content of the tile data is valid, and can be used for reliable diffing and patching of the tilesets.
