# Tools

Martin has a few additional tools that can be used to interact with the data.

## MBTiles tool
A small utility that allows users to interact with the `*.mbtiles` files from the command line. Use `mbtiles --help` to see a list of available commands, and `mbtiles <command> --help` to see help for a specific command.

This tool can be installed by compiling the latest released version with `cargo install martin-mbtiles`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

### meta-get
Retrieve raw metadata value by its name. The value is printed to stdout without any modifications.

```shell
mbtiles meta-get <file.mbtiles> <key>
```

### copy
Copy existing `.mbtiles` file to a new, non-existent file.

```shell
mbtiles copy  <src_file.mbtiles> <dst_file.mbtiles>
```
