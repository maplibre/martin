# Diffing MBTiles

## `mbtiles diff`

Copy command can also be used to compare two mbtiles files and generate a delta (diff) file. The diff file can
be [applied](#mbtiles-apply-patch) to the `src_file.mbtiles` elsewhere, to avoid copying/transmitting the entire
modified dataset. The delta file will contain all tiles that are different between the two files (modifications,
insertions, and deletions as `NULL` values), for both the tile and metadata tables.

There is one exception: `agg_tiles_hash` metadata value will be renamed to `agg_tiles_hash_after_apply`, and a
new `agg_tiles_hash` will be generated for the diff file itself. This is done to avoid confusion when applying the diff
file to the original file, as the `agg_tiles_hash` value will be different after the diff is applied. The `apply-patch`
command will automatically rename the `agg_tiles_hash_after_apply` value back to `agg_tiles_hash` when applying the
diff.

```shell
# This command will compare `file1.mbtiles` and `file2.mbtiles`, and generate a new diff file `diff.mbtiles`.
mbtiles diff file1.mbtiles file2.mbtiles diff.mbtiles

# If diff.mbtiles is applied to file1.mbtiles, it will produce file2.mbtiles 
mbtiles apply-patch file1.mbtiles diff.mbtiles file2a.mbtiles

# file2.mbtiles and file2a.mbtiles should now be the same
# Validate both files and see that their hash values are identical
mbtiles validate file2.mbtiles
[INFO ] The agg_tiles_hashes=E95C1081447FB25674DCC1EB97F60C26 has been verified for file2.mbtiles

mbtiles validate file2a.mbtiles
[INFO ] The agg_tiles_hashes=E95C1081447FB25674DCC1EB97F60C26 has been verified for file2a.mbtiles
```

## `mbtiles apply-patch`

Apply the diff file generated with the `mbtiles diff` command above to an MBTiles file. The diff file can be applied to
the `src_file.mbtiles` that has been previously downloaded to avoid copying/transmitting the entire modified dataset
again. The `src_file.mbtiles` will modified in-place. It is also possible to apply the diff file while copying the
source file to a new destination file, by using
the [`mbtiles copy --apply-patch`](mbtiles-copy.md#mbtiles-copy---apply-patch) command.

Note that the `agg_tiles_hash_after_apply` metadata value will be renamed to `agg_tiles_hash` when applying the diff.
This is done to avoid confusion when applying the diff file to the original file, as the `agg_tiles_hash` value will be
different after the diff is applied.

```shell
mbtiles apply-patch src_file.mbtiles diff_file.mbtiles
```

#### Applying diff with SQLite

Another way to apply the diff is to use the `sqlite3` command line tool directly. This SQL will delete all tiles
from `src_file.mbtiles` that are set to `NULL` in `diff_file.mbtiles`, and then insert or update all new tiles
from `diff_file.mbtiles` into `src_file.mbtiles`, where both files are of `flat` type. The name of the diff file is
passed as a query parameter to the sqlite3 command line tool, and then used in the SQL statements. Note that this does
not update the `agg_tiles_hash` metadata value, so it will be incorrect after the diff is applied.

```shell
sqlite3 src_file.mbtiles \
  -bail \
  -cmd ".parameter set @diffDbFilename diff_file.mbtiles" \
  "ATTACH DATABASE @diffDbFilename AS diffDb;" \
  "DELETE FROM tiles WHERE (zoom_level, tile_column, tile_row) IN (SELECT zoom_level, tile_column, tile_row FROM diffDb.tiles WHERE tile_data ISNULL);" \
  "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"
```
