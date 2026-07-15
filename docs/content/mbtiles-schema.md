---
icon: material/database-outline
tags:
  - mbtiles
  - tooling
---

# MBTiles Schemas

The `mbtiles` tool builds on top of the original [MBTiles specification](https://github.com/mapbox/mbtiles-spec#readme) by specifying different kinds of schema for `tiles` data. The `mbtiles` tool can convert between these schemas, and can also generate a diff between two files of any schemas, as well as merge multiple schema files into one file.

## metadata

Every schema includes a shared `metadata` table that stores key/value pairs such as the tileset name, format, and bounds.

```sql
--8<-- "files/init-metadata.sql"
```

## flat

Flat schema is the closest to the original MBTiles specification.
It stores all tiles in a single table.
This schema is the most efficient when the tileset contains no duplicate tiles.

```sql
--8<-- "files/init-flat.sql"
```

## flat-with-hash

Similar to the `flat` schema, but also includes a `tile_hash` column that contains a hash value of the `tile_data` column.
Use this schema when the tileset has no duplicate tiles, but you still want to be able to validate the content of each tile individually.

```sql
--8<-- "files/init-flat-with-hash.sql"
```

## normalized

Normalized schema is the most efficient when the tileset contains duplicate tiles.
It stores all tile blobs in the `images` table, and stores the tile Z,X,Y coordinates in a `map` table.
The `map` table contains a `tile_id` column that is a foreign key to the `images` table.
The `tile_id` column is a hash of the `tile_data` column, making it possible to both validate each individual tile like in the `flat-with-hash` schema, and also to optimize storage by storing each unique tile only once.

```sql
--8<-- "files/init-normalized.sql"
```

Optionally, `.mbtiles` files with `normalized` schema can include a `tiles_with_hash` view.
All `normalized` files created by the `mbtiles` tool will contain this view.

```sql
--8<-- "files/init-normalized-with-hash.sql"
```

### Alternative normalized schema (dedup-id)

Some tools (e.g. [Planetiler](https://github.com/onthegomap/planetiler)) produce a variation of the normalized schema that uses `tiles_shallow` and `tiles_data` tables with an integer `tile_data_id` column instead of the text-based `tile_id` (MD5 hash).

```sql
--8<-- "files/init-normalized-dedup-id.sql"
```

Since tile IDs are integers rather than content hashes, per-tile validation checks foreign key integrity (every `tile_data_id` in `tiles_shallow` must exist in `tiles_data`) instead of recomputing content hashes.
When copying from this schema to a new file, the `mbtiles` tool will produce the standard `map` + `images` normalized schema in the destination.
In our next semver major, we plan to switch this default and produce `tiles_shallow`/`tiles_data` by default as well.

## cache

The `cache` schema is similar to `normalized`, but stores extra cache metadata (`expires` and `etag`) alongside each tile.
Tile blobs are de-duplicated in the `cache_data` table, keyed by an integer `tile_id` that is the [xxh3-64](https://github.com/Cyan4973/xxHash) hash of `tile_data` (stored as an `INTEGER PRIMARY KEY`, i.e. an alias for the rowid, so identical blobs collapse to a single row).
The `tile_cache` table maps tile Z,X,Y coordinates (plus `expires`/`etag`) to a `tile_id`.
A spec-compatible `tiles` view is also created, so the file can still be read by any standard MBTiles reader (the `expires`/`etag` columns are simply invisible to it).

```sql
--8<-- "files/init-cache.sql"
```
