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
Copy an mbtiles file, optionally filtering its content by zoom levels.

```shell
mbtiles copy src_file.mbtiles dst_file.mbtiles \
        --min-zoom 0 --max-zoom 10
```

Copy command can also be used to compare two mbtiles files and generate a diff.
```shell
mbtiles copy src_file.mbtiles diff_file.mbtiles \
         --diff-with-file modified_file.mbtiles
```

This command can also be used to generate files of different [supported schema](##supported-schema).
```shell
mbtiles copy normalized.mbtiles dst.mbtiles \
         --dst-mbttype flat-with-hash
```
### apply-diff
Apply the diff file generated from `copy` command above to an mbtiles file. The diff file can be applied to the `src_file.mbtiles` elsewhere, to avoid copying/transmitting the entire modified dataset.
```shell
mbtiles apply_diff src_file.mbtiles diff_file.mbtiles
```

Another way to apply the diff is to use the `sqlite3` command line tool directly. This SQL will delete all tiles from `src_file.mbtiles` that are set to `NULL` in `diff_file.mbtiles`, and then insert or update all new tiles from `diff_file.mbtiles` into `src_file.mbtiles`. The name of the diff file is passed as a query parameter to the sqlite3 command line tool, and then used in the SQL statements.
```shell
sqlite3 src_file.mbtiles \
  -bail \
  -cmd ".parameter set @diffDbFilename diff_file.mbtiles" \
  "ATTACH DATABASE @diffDbFilename AS diffDb;" \
  "DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);" \
  "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"
```

**_NOTE:_** Both of these methods for applying a diff _only_ work for mbtiles files in the simple tables format; they do _not_ work for mbtiles files in normalized_tables format.

### validate
If the `.mbtiles` file is of `flat_with_hash` or `normalized` type, then verify that the data stored in columns `tile_hash` and `tile_id` respectively are MD5 hashes of the `tile_data` column.
```shell
mbtiles validate src_file.mbtiles
```

## Supported Schema
The `mbtiles` tool supports three different kinds of schema for `tiles` data in `.mbtiles` files:

- `flat`: 
    ```
    CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
    CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);
    ```
- `flat-with-hash`:
    ```
    CREATE TABLE tiles_with_hash (zoom_level integer NOT NULL, tile_column integer NOT NULL, tile_row integer NOT NULL, tile_data blob, tile_hash text);
    CREATE UNIQUE INDEX tiles_with_hash_index on tiles_with_hash (zoom_level, tile_column, tile_row);
    CREATE VIEW tiles AS SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;
    ```
- `normalized`:
    ```
    CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_id TEXT); 
    CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
    CREATE TABLE images (tile_data blob, tile_id text);
    CREATE UNIQUE INDEX images_id ON images (tile_id);
    CREATE VIEW tiles AS
      SELECT
          map.zoom_level AS zoom_level,
          map.tile_column AS tile_column,
          map.tile_row AS tile_row,
          images.tile_data AS tile_data
      FROM map
      JOIN images ON images.tile_id = map.tile_id;
    ```

For more general spec information, see [here](https://github.com/mapbox/mbtiles-spec#readme).