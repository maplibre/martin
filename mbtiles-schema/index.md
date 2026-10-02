# MBTiles Schemas

The `mbtiles` tool builds on top of the original [MBTiles specification](<https://github.com/mapbox/mbtiles-spec#readme>) by specifying different kinds of schema for `tiles` data. The `mbtiles` tool can convert between these schemas, and can also generate a diff between two files of any schemas, as well as merge multiple schema files into one file.

## metadata

Every schema includes a shared `metadata` table that stores key/value pairs such as the tileset name, format, and bounds.

```sql
CREATE TABLE metadata (
    name TEXT NOT NULL PRIMARY KEY,
    value TEXT
);
```

## flat

Flat schema is the closest to the original MBTiles specification. It stores all tiles in a single table. This schema is the most efficient when the tileset contains no duplicate tiles.

```sql
CREATE TABLE tiles (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_data BLOB,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
);
```

## flat-with-hash

Similar to the `flat` schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column. Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.

```sql
CREATE TABLE tiles_with_hash (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_data BLOB,
    tile_hash TEXT,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
);

CREATE VIEW tiles AS
SELECT
    zoom_level,
    tile_column,
    tile_row,
    tile_data
FROM tiles_with_hash;
```

## normalized

Normalized schema is the most efficient when the tileset contains duplicate tiles. It stores each distinct tile blob once in the `tiles_data` table, and stores the tile Z,X,Y coordinates in a `tiles_shallow` table. The `tiles_shallow` table contains a `tile_data_id` column that is a foreign key to the `tiles_data` table. [Planetiler](<https://github.com/onthegomap/planetiler>) writes the same layout.

```sql
CREATE TABLE tiles_data (
    tile_data_id INTEGER PRIMARY KEY,
    tile_data BLOB
);

CREATE TABLE tiles_shallow (
    zoom_level INTEGER,
    tile_column INTEGER,
    tile_row INTEGER,
    tile_data_id INTEGER,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
) WITHOUT ROWID;

CREATE VIEW tiles AS
SELECT
    tiles_shallow.zoom_level,
    tiles_shallow.tile_column,
    tiles_shallow.tile_row,
    tiles_data.tile_data
FROM tiles_shallow
INNER JOIN tiles_data ON tiles_shallow.tile_data_id = tiles_data.tile_data_id;
```

The `tile_data_id` column is an integer rather than a content hash, so per-tile validation checks that every `tile_data_id` in `tiles_shallow` exists in `tiles_data` instead of recomputing content hashes.

### Hash-based normalized schema

Files made by older versions of the `mbtiles` tool, and by tools like [tilelive-copy](<https://github.com/mapbox/TileLive#bintilelive-copy>), store their tile blobs in an `images` table and the tile Z,X,Y coordinates in a `map` table. The `tile_id` column that links them is a hash of the `tile_data` column, making it possible to validate each individual tile like in the `flat-with-hash` schema. The `mbtiles` tool reads these files, and a copy made without `--dst-type` keeps their schema.

```sql
CREATE TABLE map (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    tile_id TEXT,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
);

CREATE TABLE images (
    tile_id TEXT NOT NULL PRIMARY KEY,
    tile_data BLOB
);

CREATE VIEW tiles AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
```

Optionally, such files can include a `tiles_with_hash` view.

```sql
CREATE VIEW tiles_with_hash AS
SELECT
    map.zoom_level,
    map.tile_column,
    map.tile_row,
    images.tile_data,
    images.tile_id AS tile_hash
FROM map
INNER JOIN images ON map.tile_id = images.tile_id;
```

## cache

The `cache` schema is similar to `flat`, but stores extra cache metadata alongside each tile - `fetched` (when the tile was downloaded/added/last refreshed), `expires`, and `etag` - so a file can serve as a persistent web-tile cache. The `tile_cache` table holds the tile Z,X,Y coordinates, the cache metadata, and the tile blob, and a spec-compatible `tiles` view is created so the file can still be read by any standard MBTiles reader (the extra columns are simply invisible to it).

```sql
CREATE TABLE tile_cache (
    zoom_level INTEGER NOT NULL,
    tile_column INTEGER NOT NULL,
    tile_row INTEGER NOT NULL,
    fetched INTEGER,
    expires INTEGER,
    etag TEXT,
    tile_data BLOB NOT NULL,
    PRIMARY KEY (zoom_level, tile_column, tile_row)
) WITHOUT ROWID;

CREATE VIEW tiles AS
SELECT
    zoom_level,
    tile_column,
    tile_row,
    tile_data
FROM tile_cache;
```

### Supported operations

The `mbtiles` tool treats `cache` as a first-class schema, with a few deliberate restrictions:

- `summary`, `validate`, `meta-*`, and serving the file with `martin` all work. Like `flat`, there are no hashes to check during per-tile validation.
- `copy` **from** a cache file to any schema works (reading via the `tiles` view); the per-tile `fetched`/`expires`/`etag` values are dropped, since standard schemas cannot store them.
- `copy` **into** a cache file works from any schema (including `martin cp --mbtiles-type cache`); the copied entries get `NULL` `fetched`/`expires`/`etag` (unknown fetch time, never expire; identical copy runs stay byte-identical). Cache-to-cache copies preserve all cache metadata.
- `diff`, `apply-patch`, and bin-diff **into or onto** a cache file are rejected: the `NOT NULL` blob column exposed through the `tiles` view cannot represent the `NULL` "deleted tile" markers a diff needs. A cache file *can* be the compared-against or patch-source side (it is read through the view).
- `cache-purge <file> [--max-size <MB>]` removes expired entries (and optionally evicts soonest-expiring entries until the file fits the size budget), then reclaims free pages via `PRAGMA incremental_vacuum`.
