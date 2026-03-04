# Diffing MBTiles

## `mbtiles diff`

Copy command can also be used to compare two mbtiles files and generate a delta (diff) file. The diff file can
be [applied](#mbtiles-apply-patch) to the `src_file.mbtiles` elsewhere, to avoid copying/transmitting the entire
modified dataset. The delta file will contain all tiles that are different between the two files (modifications,
insertions, and deletions as `NULL` values), for both the tile and metadata tables.

```bash
# This command will compare `file1.mbtiles` and `file2.mbtiles`,
# and generate a new diff file `diff.mbtiles`.
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

### Delta file metadata

All metadata from `file2.mbtiles` will be copied to the diff file.

There are two exceptions to this. The first is that the `agg_tiles_hash` value will be renamed to `agg_tiles_hash_after_apply`. A
new `agg_tiles_hash` will be generated for the diff file itself. This is done to avoid confusion when applying the diff
file to the original file, as the `agg_tiles_hash` value will be different after the diff is applied. The `apply-patch`
command will automatically rename the `agg_tiles_hash_after_apply` value back to `agg_tiles_hash` when applying the
diff.

The second exception is that a new metadata value `agg_tiles_hash_before_apply` will be added to the diff file, which contains the
`agg_tiles_hash` value from `file1.mbtiles`. This will be used to verify that the diff file is being applied to the correct source file.

## `mbtiles apply-patch`

Apply the diff file generated with the `mbtiles diff` command above to an MBTiles file. The diff file can be applied to
the `src_file.mbtiles` that has been previously downloaded to avoid copying/transmitting the entire modified dataset
again. The `src_file.mbtiles` will modified in-place. It is also possible to apply the diff file while copying the
source file to a new destination file, by using
the [`mbtiles copy --apply-patch`](mbtiles-copy.md#mbtiles-copy---apply-patch) command.

Note that the `agg_tiles_hash_after_apply` metadata value will be renamed to `agg_tiles_hash` when applying the diff.
This is done to avoid confusion when applying the diff file to the original file, as the `agg_tiles_hash` value will be
different after the diff is applied.

```bash
mbtiles apply-patch src_file.mbtiles diff_file.mbtiles
```

#### Applying diff with SQLite

Another way to apply the diff is to use the `sqlite3` command line tool directly. This SQL will delete all tiles
from `src_file.mbtiles` that are set to `NULL` in `diff_file.mbtiles`, and then insert or update all new tiles
from `diff_file.mbtiles` into `src_file.mbtiles`, where both files are of `flat` type. The name of the diff file is
passed as a query parameter to the sqlite3 command line tool, and then used in the SQL statements. Note that this does
not update the `agg_tiles_hash` metadata value, so it will be incorrect after the diff is applied.

```bash
sqlite3 src_file.mbtiles \
  -bail \
  -cmd ".parameter set @diffDbFilename diff_file.mbtiles "\
  "ATTACH DATABASE @diffDbFilename AS diffDb; "\
  "DELETE FROM tiles "\
  "  WHERE (zoom_level, tile_column, tile_row) IN ( "\
  "    SELECT zoom_level, tile_column, tile_row "\
  "    FROM diffDb.tiles "\
  "    WHERE tile_data ISNULL); "\
  "INSERT OR REPLACE INTO tiles (zoom_level, tile_column, tile_row, tile_data) "\
  "  SELECT * FROM diffDb.tiles WHERE tile_data NOTNULL;"
```

## Binary Diff Support

The `mbtiles diff` command supports binary patching via the `--patch-type` flag,
which can significantly reduce the size of difference files.

The flag combines two choices:

- **Input type** — whether the tiles are raw or gzip-compressed
- **Output type** — whether to store whole tiles (`whole`) or binary diffs (`bin-diff`)

The key distinction is between `whole` and `bin-diff`:

| `--patch-type`  | Input      | Output                           |
| --------------- | ---------- | -------------------------------- |
| `whole`         | any        | Full tile stored as-is           |
| `bin-diff-raw`  | raw        | Brotli-compressed binary diff    |
| `bin-diff-gz`   | gzip       | Binary diff of decompressed data |

Using `bin-diff` instead of `whole` produces much smaller diff files,
because only the changed bytes between tiles are stored.

### When to use `bin-diff-gz`

Use `bin-diff-gz` when your tiles are gzip-compressed. It decompresses
each tile before computing the diff, then stores the result in a
`bsdiffrawgz` table with a `xxh3_64` hash.

Note
    `bin-diff-gz` skips `agg_tiles_hash_after_apply` validation after
    patching, because re-compressing gzip tiles may produce different bytes.
    Use `bin-diff-raw` if you need aggregate hash validation.

### Creating a diff

```bash
mbtiles diff original.mbtiles updated.mbtiles diff.mbtiles --patch-type bin-diff-raw
```

### Applying a patch

The `mbtiles` CLI automatically detects and applies binary patches
if `bsdiffraw` or `bsdiffrawgz` tables are present:

```bash
mbtiles copy diff.mbtiles original.mbtiles target.mbtiles --apply-patch
```

!!! note
    `mbtiles apply-patch` does not currently support binary patching.
    Use `mbtiles copy --apply-patch` instead.



