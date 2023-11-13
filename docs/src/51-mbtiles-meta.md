# MBTiles information and metadata

## summary
Use `mbtiles summary` to get a summary of the contents of an MBTiles file. The command will print a table with the number of tiles per zoom level, the size of the smallest and largest tiles, and the average size of tiles at each zoom level. The command will also print the bounding box of the covered area per zoom level.

```shell
File: tests/fixtures/mbtiles/world_cities.mbtiles
Schema: flat
File size: 48.00KiB
Page size: 4.00KiB
Page count: 12

|  Zoom   |  Count  |Smallest | Largest | Average |         BBox         |
|        0|        1|  1.08KiB|  1.08KiB|  1.08KiB| -180,-85,180,85      |
|        1|        4|     160B|     650B|     366B| -180,-85,180,85      |
|        2|        7|     137B|     495B|     239B| -180,-67,180,67      |
|        3|       17|      67B|     246B|     134B| -135,-41,180,67      |
|        4|       38|      64B|     175B|      86B| -135,-41,180,67      |
|        5|       57|      64B|     107B|      72B| -124,-41,180,62      |
|        6|       72|      64B|      97B|      68B| -124,-41,180,62      |
|      all|      196|      64B|   1.0KiB|      96B| -180,-85,180,85      |
```

## meta-all
Print all metadata values to stdout, as well as the results of tile detection. The format of the values printed is not stable, and should only be used for visual inspection.

```shell
mbtiles meta-all my_file.mbtiles
```

## meta-get
Retrieve raw metadata value by its name. The value is printed to stdout without any modifications.  For example, to get the `description` value from an mbtiles file:

```shell
mbtiles meta-get my_file.mbtiles description
```

## meta-set
Set metadata value by its name, or delete the key if no value is supplied. For example, to set the `description` value to `A vector tile dataset`:

```shell
mbtiles meta-set my_file.mbtiles description "A vector tile dataset"
```
