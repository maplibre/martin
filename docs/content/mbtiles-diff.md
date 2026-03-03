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

Binary Diff Support for MBTiles

The MBTiles diff command now supports binary patching, significantly reducing the size of difference files between tile sets. This is especially useful when working with large tile archives where only small portions have changed.

Quick Overview

Three patch types are available through the --patch-type flag:

Option Description Use Case
whole Original behavior - stores full tiles Simple diffs, maximum compatibility
bin-diff-raw Binary diff of raw tiles, brotli-compressed General purpose, best for most cases
bin-diff-gz Decompresses gzip tiles before diffing When tiles are gzip-compressed and you want minimal diff size

Creating Binary Diffs

Basic Diff Command

```bash
# Create a binary diff (recommended)
mbtiles diff original.mbtiles updated.mbtiles --patch-type bin-diff-raw

# Create a gzip-aware binary diff
mbtiles diff original.mbtiles updated.mbtiles --patch-type bin-diff-gz

# Traditional full-tile diff (original behavior)
mbtiles diff original.mbtiles updated.mbtiles --patch-type whole
```

Using the Alias

The mbtiles copy --diff-with-file command serves as a convenient alias:

```bash
# These commands are equivalent:
mbtiles copy --diff-with-file original.mbtiles updated.mbtiles --patch-type bin-diff-raw
mbtiles diff original.mbtiles updated.mbtiles --patch-type bin-diff-raw
```

How It Works

bin-diff-raw

· Computes binary differences between tiles
· Stores results brotli-encoded in a bsdiffraw table
· Includes xxh3_64 hash for integrity verification

bin-diff-gz

· First decompresses gzip-compressed tiles
· Computes binary differences on the decompressed data
· Stores results in a bsdiffrawgz table
· Ideal for tile sets with gzip compression where changes are small relative to decompressed size

Applying Patches

Automatic Detection

When copying with --apply-patch, Martin automatically detects and applies binary patches:

```bash
# Automatically uses bsdiffraw or bsdiffrawgz if available
mbtiles copy --apply-patch diff.mbtiles patched.mbtiles
```

The tool checks for patch tables in this order:

1. bsdiffraw (for bin-diff-raw patches)
2. bsdiffrawgz (for bin-diff-gz patches)
3. Falls back to original tiles if no patch tables exist

Important Notes

· ✅ Automatic detection works: Just use --apply-patch as before
· ⚠️ mbtiles apply-patch command does NOT yet support binary patching - use mbtiles copy --apply-patch instead
· Binary patches are significantly smaller but require more processing time

Storage Efficiency

Binary diffs can reduce storage requirements dramatically:

```bash
# Example size comparison
$ ls -lh
-rw-r--r--  diff-whole.mbtiles   850M  # Full tile storage
-rw-r--r--  diff-binary.mbtiles  120M  # Binary diff (bin-diff-raw)
-rw-r--r--  diff-binary-gz.mbtiles 95M # Gzip-aware diff (bin-diff-gz)
```

The actual savings depend on how much the data changed between versions. Small changes in large tiles yield the best compression ratios.

Technical Details

· Hashing: xxh3_64 ensures patch integrity without significant performance impact
· Compression: Brotli provides excellent compression for binary patches
· Storage: Separate tables maintain backward compatibility with older tools

References

· Pull Request #1358 - Original implementation
· bsdiff format - Binary diff algorithm
· xxHash - Fast non-cryptographic hash function
