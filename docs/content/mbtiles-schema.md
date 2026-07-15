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

The `cache` schemas store extra cache metadata (`expires` and `etag`) alongside each tile, so a file can serve as a persistent web-tile cache.
Two layouts exist, mirroring the `flat` vs `normalized` split of the regular schemas.
Both center on a `tile_cache` table holding the tile Z,X,Y coordinates and the cache metadata, and both create a spec-compatible `tiles` view so the file can still be read by any standard MBTiles reader (the `expires`/`etag` columns are simply invisible to it).

### cache-flat

The tile blob is stored inline in the `tile_cache` table.
Simple and fast, best when few tiles share the same content.

```sql
--8<-- "files/init-cache-flat.sql"
```

### cache-normalized

Tile blobs are de-duplicated in the `cache_data` table, keyed by an integer `tile_id` that is the [xxh3-64](https://github.com/Cyan4973/xxHash) hash of `tile_data` (stored as an `INTEGER PRIMARY KEY`, i.e. an alias for the rowid, so identical blobs collapse to a single row).
This is the recommended default for web-tile caches, where identical (e.g. empty or ocean) tiles are common; the `cache` CLI value is an alias for it.

```sql
--8<-- "files/init-cache-normalized.sql"
```

### Supported operations

The `mbtiles` tool treats both cache layouts as first-class schemas, with a few deliberate restrictions:

* `summary`, `validate`, `meta-*`, and serving the file with `martin` all work.
* `copy` **from** a cache file to any schema works (reading via the `tiles` view); the per-tile `expires`/`etag` values are dropped, since standard schemas cannot store them.
* `copy` **into** a cache file works from any schema (including `martin-cp --mbtiles-type cache-flat|cache-normalized`); the copied entries get `NULL` `expires`/`etag` (never expire). Copies between cache files - including across the two layouts - preserve `expires`/`etag`.
* `diff`, `apply-patch`, and bin-diff **into or onto** a cache file are rejected: the `NOT NULL` blob storage joined through the `tiles` view cannot represent the `NULL` "deleted tile" markers a diff needs. A cache file *can* be the compared-against or patch-source side (it is read through the view).
* `cache-purge <file> [--max-size <MB>]` removes expired entries (and optionally evicts soonest-expiring entries until the file fits the size budget), then reclaims free pages via `PRAGMA incremental_vacuum`.

For `cache-normalized`, per-tile validation checks foreign-key integrity (every `tile_cache.tile_id` must exist in `cache_data`) and that each blob is stored under its xxh3-64 content key, allowing for the small linear-probing window the runtime API uses on hash collisions. Unreferenced `cache_data` rows are legal - they appear when an entry is overwritten and disappear on the next purge. Like `flat`, the `cache-flat` layout has no hashes to check.

Note that bulk SQL copies into a `cache-normalized` file cannot resolve xxh3-64 collisions the way the runtime `set_cached` API does (by linear probing); the copier instead verifies afterwards that no two different blobs mapped to the same key and fails the copy in that astronomically-unlikely case.
