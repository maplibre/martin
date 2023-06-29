# Tools

Martin has a few additional tools that can be used to interact with the data.

## MBTiles tool
A small utility that allows users to interact with the `*.mbtiles` files from the command line. Use `mbtiles --help` to see a list of available commands, and `mbtiles <command> --help` to see help for a specific command.

This tool can be installed by compiling the latest released version with `cargo install martin-mbtiles`, or by downloading a pre-built binary from the [releases page](https://github.com/maplibre/martin/releases/latest).

### meta-get
Retrieve raw metadata value by its name. The value is printed to stdout without any modifications.  For example, to get the `description` value from an mbtiles file:

```shell
mbtiles meta-get my_file.mbtiles description
```

### copy
Copy an mbtiles file, optionally filtering its content by zoom levels. Can also flatten mbtiles file from de-duplicated tiles to a simple table structure.

```shell
mbtiles copy src_file.mbtiles dst_file.mbtiles --min-zoom 0 --max-zoom 10 --force-simple
```

This command can also be used to compare two mbtiles files and generate a diff.
```shell
mbtiles copy src_file.mbtiles diff_file.mbtiles --force-simple --diff-with-file modified_file.mbtiles
```
* The `diff_file.mbtiles` can then be applied to the `src_file.mbtiles` elsewhere, to avoid copying large files when only small updates are needed. To do this, you may want to copy and then use [apply_diff.sh](/tests/fixtures/apply_diff.sh) as follows.

        ./apply_diff.sh src_file.mbtiles diff_file.mbtiles
  **_NOTE:_** This _only_ works for mbtiles files in the simple tables format; it does _not_ work for mbtiles files in deduplicated format.
