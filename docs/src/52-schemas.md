## Supported Schema
The `mbtiles` tool supports three different kinds of schema for `tiles` data in `.mbtiles` files. See also the original [specification](https://github.com/mapbox/mbtiles-spec#readme).

### flat
```sql, ignore
CREATE TABLE tiles (zoom_level integer, tile_column integer, tile_row integer, tile_data blob);
CREATE UNIQUE INDEX tile_index on tiles (zoom_level, tile_column, tile_row);
```

### flat-with-hash
```sql, ignore
CREATE TABLE tiles_with_hash (
  zoom_level integer NOT NULL,
  tile_column integer NOT NULL,
  tile_row integer NOT NULL,
  tile_data blob,
  tile_hash text);
CREATE UNIQUE INDEX tiles_with_hash_index on tiles_with_hash (zoom_level, tile_column, tile_row);
CREATE VIEW tiles AS SELECT zoom_level, tile_column, tile_row, tile_data FROM tiles_with_hash;
```

### normalized
```sql, ignore
CREATE TABLE map (zoom_level INTEGER, tile_column INTEGER, tile_row INTEGER, tile_id TEXT);
CREATE UNIQUE INDEX map_index ON map (zoom_level, tile_column, tile_row);
CREATE TABLE images (tile_id text, tile_data blob);
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

Optionally, `.mbtiles` files with `normalized` schema can include a `tiles_with_hash` view:

```sql, ignore
CREATE VIEW tiles_with_hash AS
  SELECT
      map.zoom_level AS zoom_level,
      map.tile_column AS tile_column,
      map.tile_row AS tile_row,
      images.tile_data AS tile_data,
      images.tile_id AS tile_hash
  FROM map LEFT JOIN images ON map.tile_id = images.tile_id;
```

**__Note:__** All `normalized` files created by the `mbtiles` tool will contain this view.
