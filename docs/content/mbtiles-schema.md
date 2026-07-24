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

The `cache` schema is similar to `flat`, but stores extra cache metadata alongside each tile - `fetched` (when the tile was downloaded/added/last refreshed), `expires`, and `etag` - so a file can serve as a persistent web-tile cache.
The `tile_cache` table holds the tile Z,X,Y coordinates, the cache metadata, and the tile blob, and a spec-compatible `tiles` view is created so the file can still be read by any standard MBTiles reader (the extra columns are simply invisible to it).

```sql
--8<-- "files/init-cache.sql"
```

### Supported operations

The `mbtiles` tool treats `cache` as a first-class schema, with a few deliberate restrictions:

* `summary`, `validate`, `meta-*`, and serving the file with `martin` all work. Like `flat`, there are no hashes to check during per-tile validation.
* `copy` **from** a cache file to any schema works (reading via the `tiles` view); the per-tile `fetched`/`expires`/`etag` values are dropped, since standard schemas cannot store them.
* `copy` **into** a cache file works from any schema (including `martin-cp --mbtiles-type cache`); the copied entries get `NULL` `fetched`/`expires`/`etag` (unknown fetch time, never expire; identical copy runs stay byte-identical). Cache-to-cache copies preserve all cache metadata.
* `diff`, `apply-patch`, and bin-diff **into or onto** a cache file are rejected: the `NOT NULL` blob column exposed through the `tiles` view cannot represent the `NULL` "deleted tile" markers a diff needs. A cache file *can* be the compared-against or patch-source side (it is read through the view).
* `cache-purge <file> [--max-size <MB>]` removes expired entries (and optionally evicts soonest-expiring entries until the file fits the size budget), then reclaims free pages via `PRAGMA incremental_vacuum`.
