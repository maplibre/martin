# MBTiles Schemas

The `mbtiles` tool builds on top of the original [MBTiles specification](https://github.com/mapbox/mbtiles-spec#readme) by specifying three different kinds of schema for `tiles` data: `flat`, `flat-with-hash`, and `normalized`. The `mbtiles` tool can convert between these schemas, and can also generate a diff between two files of any schemas, as well as merge multiple schema files into one file.

## flat

Flat schema is the closest to the original MBTiles specification. It stores all tiles in a single table. This schema is the most efficient when the tileset contains no duplicate tiles.

```sql, ignore
{{#include ../../mbtiles/sql/init-flat.sql}}
```

## flat-with-hash

Similar to the `flat` schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column. Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.

```sql, ignore
{{#include ../../mbtiles/sql/init-flat-with-hash.sql}}
```

## normalized

Normalized schema is the most efficient when the tileset contains duplicate tiles. It stores all tile blobs in the `images` table, and stores the tile Z,X,Y coordinates in a `map` table. The `map` table contains a `tile_id` column that is a foreign key to the `images` table. The `tile_id` column is a hash of the `tile_data` column, making it possible to both validate each individual tile like in the `flat-with-hash` schema, and also to optimize storage by storing each unique tile only once.

```sql, ignore
{{#include ../../mbtiles/sql/init-normalized.sql}}
```

Optionally, `.mbtiles` files with `normalized` schema can include a `tiles_with_hash` view. All `normalized` files created by the `mbtiles` tool will contain this view.

```sql, ignore
{{#include ../../mbtiles/sql/init-normalized.sql:26:}}
```
