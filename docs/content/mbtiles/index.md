# Working with MBTiles archives

Martin includes `mbtiles` utility to interact with the [`*.mbtiles` files](mbtiles-schema.md) from the command line.
It allows users to [examine](mbtiles-meta.md), [copy](mbtiles-copy.md), [validate](mbtiles-validation.md) or [compare and apply diffs between them](mbtiles-diff.md).

This tool can be installed by compiling the latest released version with `cargo install mbtiles --locked`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

Use `mbtiles --help` to see a list of available commands:

```text
--8<-- "help/mbtiles.txt"
```

And `mbtiles <command> --help` to see help for a specific command.
Example for `mbtiles validate --help`:

```text
--8<-- "help/mbtiles-validate.txt"
```
