# Diffing MBTiles

## `mbtiles diff`

Diff command compares two mbtiles files `A` and `B`, and generates a diff (delta) file.
If the diff file is [applied](mbtiles-copy.md#mbtiles-apply-patch) to `A`, it will produce `B`.  
The diff file will contain all tiles that are different between the two files
(modifications, insertions, and deletions as `NULL` values), for both the tile and metadata tables.  
The only exception is `agg_tiles_has` metadata value. It will be renamed to `agg_tiles_hash_in_diff` and a
new `agg_tiles_hash` will be generated for the diff file itself.

```shell
# This command will comapre `a.mbtiles` and `b.mbtiles`, and generate a new diff file `diff.mbtiles`.
# If diff.mbtiles is applied to a.mbtiles, it will produce b.mbtiles 
mbtiles diff a.mbtiles b.mbtiles diff.mbtiles
```
