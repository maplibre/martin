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

Copy command can also be used to compare two mbtiles files and generate a diff.
```shell
mbtiles copy src_file.mbtiles diff_file.mbtiles --force-simple --diff-with-file modified_file.mbtiles
```

The `diff_file.mbtiles` can be applied to the `src_file.mbtiles` elsewhere to avoid copying/transmitting the entire modified dataset.

One way to apply the diff is to use the `sqlite3` command line tool directly. Here, we assume that the `src_file.mbtiles` is in the simple tables format, and that the `diff_file.mbtiles` is the output of the `mbtiles copy` command above. This SQL will delete all tiles from `src_file.mbtiles` that are set to `NULL` in `diff_file.mbtiles`, and then insert or update all new tiles from `diff_file.mbtiles` into `src_file.mbtiles`. The name of the diff file is passed as a query parameter to the sqlite3 command line tool, and then used in the SQL statements.

```shell
sqlite3 src_file.mbtiles \
  -bail \
  -cmd ".parameter set @diffDbFilename diff_file.mbtiles" \
  "ATTACH DATABASE @diffDbFilename AS diffDb;" \
  "DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);" \
  "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"
```
