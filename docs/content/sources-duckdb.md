---
icon: simple/duckdb
tags:
  - duckdb
  - geoparquet
  - tile-sources
  - configuration
---

# DuckDB Sources

!!! warning
    This feature is currently unstable and thus not included in the default build.
    Its behavior may change in patch releases.

    To experiment with it on any supported platform, [install Rust](https://rust-lang.org/tools/install/), and run this to download, compile, and install Martin with the unstable feature:

    ```bash
    cargo install martin --locked --features=unstable-duckdb
    ```

    It is unstable due to the limitations of our current implementation:

    - DuckDB sources are not included in default binaries, Homebrew, or the Docker image
    - Local GeoParquet must be a single file; directories and globs are rejected
    - Hot reload is not implemented
    - MLT postprocessing is not supported
    - The published configuration schema does not yet include DuckDB sources

    We welcome contributions to help stabilize this feature!

Martin serves vector tiles from [GeoParquet](https://geoparquet.org/) files and from tables and macros in [DuckDB](https://duckdb.org/) database files.

Pass a local file to use the default settings:

- `martin data.parquet` serves a GeoParquet file.
- `martin tiles.duckdb` publishes the database's geometry tables and `(z, x, y)` tile macros.

To customize DuckDB sources or use remote GeoParquet, use a [configuration file](config-file/index.md) and run `martin --config config.yaml`.

Add `--save-config resolved-config.yaml` to any of these commands to save the configuration with resolved per-source defaults:

```bash
martin --config config.yaml --save-config resolved-config.yaml
```

## Run Martin with configuration file

```yaml
# DuckDB / GeoParquet sources (requires --features=unstable-duckdb)
duckdb:
  # Connection pool size used by DuckDB sources unless overridden per-source. [default: 4]
  pool_size: 4
  # Optional DuckDB execution thread count for each connection.
  threads: 2
  # Optional DuckDB memory limit in megabytes for each connection.
  memory_limit_mb: 1024
  # Specify how bounds should be computed [default: quick]
  #
  # Options:
  # - `calc` compute geometry bounds on startup.
  # - `quick` same as 'calc', but the calculation will be aborted after 5 seconds.
  # - `skip` does not compute geometry bounds on startup.
  auto_bounds: quick
  sources:
    # Local GeoParquet file, published as source id `buildings`
    - geoparquet: /data/buildings.parquet
      layer_id: buildings
      geometry_column: geom
      srid: 4326
      minzoom: 0
      maxzoom: 14
      extent: 4096
      buffer: 64
      clip_geom: true
    # Remote GeoParquet over HTTP(S)
    - geoparquet: https://example.org/data/places.parquet
      layer_id: places
    # Remote GeoParquet in an object store, optionally a glob over many part files
    - geoparquet: s3://overturemaps-us-west-2/release/2026-08-19.0/theme=places/type=place/*.parquet
      layer_id: overture_places
      geometry_column: geometry
      srid: 4326
```

The top-level `pool_size`, `threads`, `memory_limit_mb`, and `auto_bounds` apply to every DuckDB source unless overridden on that source:

- **`pool_size`** - connection pool size per source (defaults to `4`)
- **`threads`** - DuckDB thread count per connection. When unset, DuckDB uses its own default.
- **`memory_limit_mb`** - DuckDB memory limit in megabytes per connection.
- **`auto_bounds`** - how TileJSON bounds are computed (defaults to `quick`):
  - **`quick`** - compute geometry bounds, but abort if it takes longer than 5 seconds
  - **`calc`** - compute geometry bounds. The startup time may be significant.
  - **`skip`** - do not compute bounds. TileJSON will omit `bounds`.

Each GeoParquet source supports:

- **`geoparquet`**
  - a remote URL of the GeoParquet file (`http`, `https`, `s3`, `gs`, `gcs`, `r2`, `az`, `azure`, `abfss` and `hf` URLs). A remote URL may be a glob such as `s3://bucket/prefix/*.parquet`, which DuckDB expands into every matching part file.
  - a local path or `file://` URLs. Local paths must name a single file.
- **`layer_id`** - MVT `source-layer` and the base for the source id (defaults to the file or URL stem).
- **`geometry_column`** - geometry column name. Auto-detected when the file has exactly one geometry column.
- **`id_column`** - optional table column to use as the MVT feature id.
- **`srid`** - source SRID. Auto-detected via `ST_CRS` when omitted. Non-positive values are treated as unset.
- **`minzoom`** / **`maxzoom`** - optional zoom range advertised in TileJSON.
- **`extent`** - side length of the MVT tile coordinate grid each tile is encoded into (defaults to `4096`, the value [MapLibre](https://maplibre.org/) assumes). Must be non-zero.
- **`buffer`** - clip margin kept around each tile edge, in tile units (defaults to `64`). Increase it if you see seam artifacts on line caps/joins or polygon outlines near tile edges.
- **`clip_geom`** - controls if geometries should be clipped or encoded as is (defaults to `true`).

Per-source `pool_size`, `threads`, `memory_limit_mb`, and `auto_bounds` override the top-level values for that source.

!!! tip
    See [our tile sources explanation](sources-tiles/index.md) for a more detailed explanation on the difference between our available data sources.
    DuckDB sources can be combined with other sources via [Composite Sources](sources-composite.md).

!!! note
    SRID auto-detection supports EPSG codes and `OGC:CRS84` only.
    If the file has more than one geometry column, set `geometry_column` explicitly.

!!! tip "Row-group pruning"
    To reduce the IO necessary on large duckdb queries, we use can use the [GeoParquet 1.1 `covering`](https://geoparquet.org/releases/v1.1.0/#covering) declaration out of the file's `geo` metadata on startup and honor `bbox.{x,y}{min,max}` structs.

    Pruning is also skipped when the source SRID is neither `4326` nor `3857`, because transforming a tile envelope into another projection can under-cover it and silently clip features at tile edges.

!!! warning "Vector tiles can only carry text, numeric and boolean properties"
    Martin casts every other scalar column - dates, timestamps, `DECIMAL`, `UUID`, `ENUM`, and small or unsigned integers - to the nearest type MVT supports.
    Columns with no MVT representation at all, such as `STRUCT`, `LIST`, `MAP` and `BLOB`, are dropped and named in a startup warning.
    The TileJSON `vector_layers[].fields` reports the type each property is served as, not the type it has on disk.

## Database sources

A `database:` entry names a DuckDB database file, which Martin opens read-only.
One connection pool serves every source of that file.
Tables with a `GEOMETRY` column are served as MVT layers the same way GeoParquet files are, and table macros taking `(z, x, y)` are served as ready-made tiles.

```yaml
duckdb:
  pool_size: 4
  auto_bounds: quick
  sources:
    # Publish every geometry table and (z, x, y) macro of the file
    - database: /data/tiles.duckdb
    # Or pick what to publish
    - database: /data/more_tiles.duckdb
      auto_publish:
        # Optionally limit both tables and macros to these schemas
        from_schemas: [main, places]
        tables:
          # Add more schemas to the ones listed above
          from_schemas: roads
          # How the source id is built from the schema, the table and its geometry column [default: "{table}"]
          source_id_format: "{schema}.{table}"
          # The first integer column of this list becomes the MVT feature id
          id_columns: [id, gid]
          extent: 4096
          buffer: 64
          clip_geom: true
        macros:
          from_schemas: main
          # How the source id is built from the schema and the macro name [default: "{macro}"]
          source_id_format: "{schema}.{macro}"
      tables:
        # Source id
        roads:
          # Schema the table lives in [default: main]
          schema: main
          table: roads
          # Same options as a GeoParquet source
          geometry_column: geom
          id_column: id
          srid: 4326
          minzoom: 0
          maxzoom: 14
          extent: 4096
          buffer: 64
          clip_geom: true
      macros:
        # Source id
        roads_at_zoom:
          schema: main
          macro: roads_mvt
          minzoom: 0
          maxzoom: 14
          # Bounds in WGS84 [left, bottom, right, top]; not computed for macros
          bounds: [-180, -85, 180, 85]
```

`auto_publish` follows the [PostgreSQL rules](sources-pg-tables.md):

- A bare `database:` entry publishes every geometry table and `(z, x, y)` macro. - Configuring `tables` or `macros` explicitly turns discovery off unless `auto_publish` is set.
- Inside `auto_publish`, mentioning only one of `tables` or `macros` disables the other.
- A table with several geometry columns yields one source per column.
- Discovered tables detect their SRID from the column's CRS, so store it with `GEOMETRY('EPSG:4326')` or set `srid` on an explicit table.

A macro is any `CREATE MACRO name(z, x, y) AS TABLE ...` whose first row's first column is the tile, for example:

```sql
CREATE MACRO roads_mvt(z, x, y) AS TABLE
SELECT ST_AsMVT(tile, 'roads', 4096, 'geom') AS mvt
FROM (
    SELECT
        ST_AsMVTGeom(
            ST_Transform(geom, 'EPSG:4326', 'EPSG:3857', always_xy := true),
            ST_Extent(ST_TileEnvelope(z, x, y)),
            4096, 64, true
        ) AS geom,
        id,
        name
    FROM roads
    WHERE ST_Intersects(
        ST_Transform(geom, 'EPSG:4326', 'EPSG:3857', always_xy := true),
        ST_TileEnvelope(z, x, y)
    )
) AS tile;
```

Macros are served as-is: Martin does not compute their bounds or `vector_layers`.

## About GeoParquet

[GeoParquet](https://geoparquet.org/) is a [Parquet](https://parquet.apache.org/) file with geospatial metadata.

Parquet is a columnar file format.
GeoParquet adds a standard way to store geometry columns and CRS information inside that file.

Martin reads GeoParquet with DuckDB `read_parquet`, loads the DuckDB `spatial` extension, and generates MVT tiles on each request.
Remote URLs also load the DuckDB `httpfs` extension.

Martin passes remote URLs to DuckDB unchanged and does not manage credentials for them.
Public buckets work with no further configuration.
Private ones need DuckDB's own credential configuration, such as the standard AWS environment variables.

You may want to visit these specs:

- [GeoParquet](https://geoparquet.org/)
- [Parquet](https://parquet.apache.org/docs/)
- [DuckDB spatial](https://duckdb.org/docs/stable/extensions/spatial/overview)
