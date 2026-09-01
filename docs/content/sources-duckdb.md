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
    - There is no CLI shorthand for `.parquet` or `.duckdb` files
    - DuckDB database sources (`database:`) are not yet supported
    - Local GeoParquet must be a file; directories are rejected
    - Remote GeoParquet is `http://` / `https://` only
    - Hot reload is not implemented
    - MLT postprocessing is not supported
    - The published configuration schema does not yet include DuckDB sources

    We welcome contributions to help stabilize this feature!

Martin can serve vector tiles on the fly from [GeoParquet](https://geoparquet.org/) files via [DuckDB](https://duckdb.org/).
Instead of incurring the overhead of serving them directly, we serve them as vector tiles.

DuckDB sources are only available via the [configuration file](config-file/index.md).
There is no CLI shorthand.
Create a configuration file and start Martin with `martin --config config.yaml`.
Once a DuckDB configuration exists, `martin --config config.yaml --save-config resolved-config.yaml` writes a copy with the resolved per-source defaults.

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

- **`geoparquet`** - local path or `http://` / `https://` URL of the GeoParquet file.
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

!!! note
    Vector tiles can only carry text, numeric and boolean properties.
    Martin casts every other scalar column - dates, timestamps, `DECIMAL`, `UUID`, `ENUM`, and small or unsigned integers - to the nearest type MVT supports.
    Columns with no MVT representation at all, such as `STRUCT`, `LIST`, `MAP` and `BLOB`, are dropped and named in a startup warning.
    The TileJSON `vector_layers[].fields` reports the type each property is served as, not the type it has on disk.

## Database sources

A `database:` entry names a DuckDB database file.
The configuration parser accepts the `auto_publish`, `tables`, and `macros` blocks shown below, but does not interpret them yet.
Database sources are not yet supported; Martin logs a warning and skips the entire entry.

```yaml
# Keep serving the working GeoParquet sources when the database source is skipped.
on_invalid: warn
duckdb:
  pool_size: 4
  auto_bounds: quick
  sources:
    - database: /data/tiles.duckdb
      auto_publish:
        tables:
          from_schemas: autodetect
          source_id_format: "{table}"
          id_columns: [id, gid]
          extent: 4096
          buffer: 64
          clip_geom: true
      tables:
        roads:
          schema: main
          table: roads
          geometry_column: geom
          srid: 4326
          minzoom: 0
          maxzoom: 14
          properties:
            id: int4
            name: varchar
```

Without `on_invalid: warn`, the default `abort` policy stops Martin because the database entry cannot be resolved.
Do not rely on any of the database source options above yet; they are included to show the accepted configuration shape only.

## About GeoParquet

[GeoParquet](https://geoparquet.org/) is a [Parquet](https://parquet.apache.org/) file with geospatial metadata.

Parquet is a columnar file format.
GeoParquet adds a standard way to store geometry columns and CRS information inside that file.

Martin reads GeoParquet with DuckDB `read_parquet`, loads the DuckDB `spatial` extension, and generates MVT tiles on each request.
Remote `http://` / `https://` URLs also load the DuckDB `httpfs` extension.

You may want to visit these specs:

- [GeoParquet](https://geoparquet.org/)
- [Parquet](https://parquet.apache.org/docs/)
- [DuckDB spatial](https://duckdb.org/docs/stable/extensions/spatial/overview)
