# Diffing MBTiles

## `mbtiles diff`

Diff command compares two mbtiles files `file A` and `file B`, and generates a delta (diff) file. If the diff file
is [applied](mbtiles-copy.md#mbtiles-apply-patch) to `A`, it will produce `B`.  
The delta file will contain all tiles that are different between the two files (modifications, insertions, and deletions
as `NULL` values), for both the tile and metadata tables.  
The only exception is `agg_tiles_has` metadata value. It will be renamed to `agg_tiles_hash_in_diff` and a
new `agg_tiles_hash` will be generated for the diff file itself.

```shell
# This command will comapre `file_a.mbtiles` and `file_b.mbtiles`, and generate a new diff file `diff_result.mbtiles`,This command will compares file_a.mbtiles and file_b.mbtiles, and generates a new diff file diff_result.mbtiles 
# If the diff file is applied to file_a, it will produce file_b. 
mbtiles diff file_a.mbtiles file_b.mbtiles diff_result.mbtiles
```
