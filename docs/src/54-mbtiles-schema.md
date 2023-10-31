# MBTiles Schemas
The `mbtiles` tool builds on top of the original [MBTiles specification](https://github.com/mapbox/mbtiles-spec#readme) by specifying three different kinds of schema for `tiles` data: `flat`, `flat-with-hash`, and `normalized`. The `mbtiles` tool can convert between these schemas, and can also generate a diff between two files of any schemas, as well as merge multiple schema files into one file.

## flat
Flat schema is the closest to the original MBTiles specification. It stores all tiles in a single table. This schema is the most efficient when the tileset contains no duplicate tiles.

```sql, ignore
CREATE TABLE tiles (
    zoom_level  INTEGER,
    tile_column INTEGER,
    tile_row    INTEGER,
    tile_data   BLOB);

CREATE UNIQUE INDEX tile_index on tiles (
    zoom_level, tile_column, tile_row);
```

## flat-with-hash
Similar to the `flat` schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column. Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.

```sql, ignore
CREATE TABLE tiles_with_hash (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_data BLOB,
    tile_hash TEXT);

CREATE UNIQUE INDEX tiles_with_hash_index on tiles_with_hash (
    zoom_level, tile_column, tile_row);

CREATE VIEW tiles AS
    SELECT zoom_level, tile_column, tile_row, tile_data
    FROM tiles_with_hash;
```

## normalized
Normalized schema is the most efficient when the tileset contains duplicate tiles. It stores all tile blobs in the `images` table, and stores the tile Z,X,Y coordinates in a `map` table. The `map` table contains a `tile_id` column that is a foreign key to the `images` table. The `tile_id` column is a hash of the `tile_data` column, making it possible to both validate each individual tile like in the `flat-with-hash` schema, and also to optimize storage by storing each unique tile only once.

```sql, ignore
CREATE TABLE map (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_id TEXT);

CREATE TABLE images (
    tile_id TEXT,
    tile_data BLOB);

CREATE UNIQUE INDEX map_index ON map (
    zoom_level, tile_column, tile_row);
CREATE UNIQUE INDEX images_id ON images (
    tile_id);

CREATE VIEW tiles AS
    SELECT
        map.zoom_level AS zoom_level,
        map.tile_column AS tile_column,
        map.tile_row AS tile_row,
        images.tile_data AS tile_data
    FROM
        map JOIN images
        ON images.tile_id = map.tile_id;
```

Optionally, `.mbtiles` files with `normalized` schema can include a `tiles_with_hash` view. All `normalized` files created by the `mbtiles` tool will contain this view.

```sql, ignore
CREATE VIEW tiles_with_hash AS
    SELECT
        map.zoom_level AS zoom_level,
        map.tile_column AS tile_column,
        map.tile_row AS tile_row,
        images.tile_data AS tile_data,
        images.tile_id AS tile_hash
    FROM
        map JOIN images
        ON map.tile_id = images.tile_id;
```
