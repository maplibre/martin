# MBTiles Schemas

The `mbtiles` tool builds on top of the original [MBTiles specification](https://github.com/mapbox/mbtiles-spec#readme) by specifying three different kinds of schema for `tiles` data: `flat`, `flat-with-hash`, and `normalized`. The `mbtiles` tool can convert between these schemas, and can also generate a diff between two files of any schemas, as well as merge multiple schema files into one file.

## flat

Flat schema is the closest to the original MBTiles specification. It stores all tiles in a single table. This schema is the most efficient when the tileset contains no duplicate tiles.

```sql
--8<-- "files/init-flat.sql"
```

## flat-with-hash

Similar to the `flat` schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column. Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.

```sql
--8<-- "files/init-flat-with-hash.sql"
```

## normalized-with-image

Normalized schema is the most efficient when the tileset contains duplicate tiles. It stores all tile blobs in the `images` table, and stores the tile Z,X,Y coordinates in a `map` table. The `map` table contains a `tile_id` column that is a foreign key to the `images` table. The `tile_id` column is a hash of the `tile_data` column, making it possible to both validate each individual tile like in the `flat-with-hash` schema, and also to optimize storage by storing each unique tile only once.

```sql
--8<-- "files/init-normalized.sql:0:28"
```

Optionally, `.mbtiles` files with `normalized` schema can include a `tiles_with_hash` view. All `normalized` files created by the `mbtiles` tool will contain this view.

```sql
--8<-- "files/init-normalized.sql:30:39"
```

## normalized-with-view

This is an alternative normalized schema produced by [Planetiler](https://github.com/onthegomap/planetiler). Like the `normalized` schema, it deduplicates tiles, but uses `tiles_shallow` and `tiles_data` tables with integer IDs instead of `map` and `images` tables with text MD5 hash IDs. Tile data is accessible through a `tiles` view.

Unlike the `normalized` schema, this variant uses integer `tile_data_id` keys (the primary key in `tiles_data`) rather than MD5 hashes of tile bytes.

The `mbtiles` tool supports reading (summary, verification, tile serving) from `normalized-with-view` files. Writing to this format is not supported; use `mbtiles copy` to convert to another schema.

```sql
--8<-- "files/init-normalized-with-view.sql"
```
