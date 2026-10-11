# Source Architecture

How each kind of source is discovered, loaded and asked for tiles, and how the catalog follows changes without a restart. See [Serving Architecture](<https://maplibre.org/martin/architecture/index.md>) for how the server uses them.

```
graph TB
    Martin[Martin Server]
    Pool[Connection Pool<br/>deadpool-postgres]

    subgraph PostgreSQL
        Tables[Tables with<br/>Geometry Columns]
        Functions[MVT Functions]
        PostGIS[PostGIS Extension]
    end

    Martin --> Pool
    Pool --> Tables
    Pool --> Functions
    Tables --> PostGIS
    Functions --> PostGIS
```

**How it works**:

1. Martin connects to PostgreSQL using connection string
2. Queries `geometry_columns` view to discover tables
3. Queries `pg_proc` to discover MVT-returning functions
4. Publishes them through the Reload Driver, which re-runs discovery every `reload_interval`
5. Maintains connection pool for efficient query execution
6. Generates tile SQL queries with bbox parameters
7. Returns results as MVT tiles

```
graph TB
    Martin[Martin Server]

    subgraph "Local Files"
        MBT[MBTiles<br/>SQLite]
        PMT[PMTiles<br/>Binary Format]
        COG[GeoTIFF<br/>Cloud Optimized]
    end

    subgraph "Remote Files"
        S3MBT[S3/Azure/GCP<br/>MBTiles]
        S3PMT[S3/Azure/GCP<br/>PMTiles]
        S3COG[S3/Azure/GCP<br/>GeoTIFF]
    end

    Martin --> MBT
    Martin --> PMT
    Martin --> COG
    Martin --> S3MBT
    Martin --> S3PMT
    Martin --> S3COG
```

**How it works**:

1. Martin scans configured directories for tile files
2. Opens MBTiles with SQLite (using `sqlx`)
3. Opens PMTiles with custom parser (HTTP range requests for remote)
4. Opens COG with TIFF parser (HTTP range requests for remote)
5. Uses `object_store` crate for S3/Azure/GCP access
6. Serves tiles directly from file format

Unlike the other file sources, GeoJSON files hold raw geometry rather than pre-baked tiles. Martin loads the whole file once, reprojects and indexes it in memory, then clips and encodes an MVT tile on every request.

```
graph TB
    subgraph Load["Load time (once per source)"]
        File[".geojson / .json file"]
        Parse["Parse (geojson crate)<br/>-> geo_types::Geometry"]
        Reproject["Reproject WGS84 -> WebMercator"]
        RTree["Packed Hilbert R-tree<br/>(geo_index)"]
    end

    subgraph Request["Per tile request"]
        Rect["Tile Rect + buffer<br/>(WebMercator bbox)"]
        Query["R-tree candidate lookup"]
        Clip["Parallel clip + transform<br/>(geo, rayon)"]
        Encode["MVT encode<br/>(fast-mvt)"]
    end

    File --> Parse --> Reproject --> RTree
    RTree -.->|in-memory features| Query
    Rect --> Query --> Clip --> Encode
```

**Load time** (`GeoJsonSource::new`):

1. The entire file is read into memory and parsed with the `geojson` crate; each feature's geometry becomes a `geo_types::Geometry<f64>`.
2. Every geometry is reprojected once from WGS84 to Web Mercator, so all per-tile work stays in Mercator.
3. A packed Hilbert R-tree (`geo_index`) is built over the feature bounding boxes for fast spatial lookup, and the union of all boxes becomes the TileJSON `bounds`.
4. Features (Mercator geometry + JSON properties) are held in memory for the lifetime of the source.

**Per request** (`GeoJsonSource::get_tile`):

1. The requested `z/x/y` is converted to a Web Mercator tile `Rect`, grown outward by the configurable `buffer` (as a `buffer / extent` fraction) so geometry near the edge survives clipping.
2. The R-tree is queried for candidate features overlapping the buffered rect; no candidates yields an empty tile (`204 No Content`).
3. Candidates are clipped in parallel (`rayon`): polygons via boolean intersection, lines via line-clip, points via containment - then snapped, repaired, re-oriented, and transformed into the tile's integer coordinate grid (`extent` units, y flipped for MVT).
4. Surviving features are encoded into a single MVT layer named after the source id (`fast-mvt`); properties are mapped to MVT values (JSON arrays/objects serialized to strings, nulls dropped).
5. The tile is returned uncompressed as `application/x-protobuf`; the shared server layer then handles caching, ETag/`304`, and `Content-Encoding`, identically to every other source.

**Configuration** (`GeoJsonConfig`, both `.json` and `.geojson` extensions are discovered):

- `extent` (default `4096`) - side length of the MVT integer coordinate grid a tile is encoded into. A `NonZeroU32`, so `0` is rejected at parse time.
- `buffer` (default `64`, in tile units) - margin of geometry kept around each tile edge to avoid seams between neighboring tiles.

Hot-reload uses the same generic Reload Driver as the other file sources (see below), watching the configured directories for added, changed, or removed `.geojson`/`.json` files.

Unlike tile archives, GeoParquet files hold raw geometry rather than pre-baked tiles. Martin uses DuckDB to read the parquet file, clip geometry to the requested tile, and encode an MVT tile on every request.

```
graph TB
    subgraph Load["Load time (once per source)"]
        File["GeoParquet file or remote URL"]
        Pool["DuckDBPool<br/>in-memory + spatial"]
        Describe["DESCRIBE read_parquet"]
        CRS["ST_CRS / configured SRID"]
        Bounds["auto_bounds<br/>ST_Extent_Approx / ST_Extent"]
    end

    subgraph Request["Per tile request"]
        XYZ["z / x / y"]
        SQL["ST_Transform + ST_AsMVTGeom"]
        MVT["ST_AsMVT"]
    end

    File --> Pool --> Describe --> CRS --> Bounds
    Pool -.->|pooled connection| SQL
    XYZ --> SQL --> MVT
```

**GeoParquet load time** (`DuckDBSource`):

1. Martin creates a DuckDB connection pool. Local GeoParquet uses an in-memory database with the `spatial` extension; remote `http(s)` URLs also load `httpfs`.
2. Columns are discovered with `DESCRIBE SELECT * FROM read_parquet(...)`. The geometry column is taken from config, or auto-detected when there is exactly one geometry column.
3. SRID is taken from config, or from `ST_CRS` on the first non-null geometry (EPSG and `OGC:CRS84` only).
4. TileJSON bounds are computed according to `auto_bounds` (`quick`, `calc`, or `skip`).
5. An MVT SQL query is built once and executed per tile with `z`, `x`, `y` parameters.

**GeoParquet requests** (`DuckDBSource::get_tile`):

1. A connection is taken from the pool and the tile query runs on a blocking thread.
2. Geometry is stamped with the source CRS, transformed to Web Mercator (`EPSG:3857`), filtered to the tile envelope (expanded by `buffer`), clipped with `ST_AsMVTGeom`, and encoded with `ST_AsMVT`.
3. Non-geometry columns become MVT feature properties. `id_column`, if set, becomes the MVT feature id.
4. The tile is returned uncompressed as `application/x-protobuf`; the shared server layer then handles caching, ETag/`304`, and `Content-Encoding`, identically to every other source.

**Database files** are opened read-only. Each `database` entry creates one connection pool shared by its table and macro sources. Geometry tables use the same MVT generation path as GeoParquet; `(z, x, y)` table macros return ready-made tiles.

**Configuration** (`DuckDbConfig`; requires `--features=unstable-duckdb`):

`martin data.parquet` and `martin tiles.duckdb` use the default settings. The database command publishes geometry tables and `(z, x, y)` tile macros. Use a [configuration file](<https://maplibre.org/martin/config-file/index.md>) for remote GeoParquet or custom settings:

- `pool_size` (default `4`) - connection pool size per GeoParquet source or database entry.
- `auto_bounds` (default `quick`) - how TileJSON bounds are computed.
- Per-source `geoparquet` path or URL, plus optional `layer_id`, `geometry_column`, `srid`, `extent`, `buffer`, and `clip_geom`.
- `database` file path, with optional `auto_publish`, `tables`, and `macros` to control which sources are published. See [DuckDB database sources](<https://maplibre.org/martin/sources-duckdb/#database-sources>).

DuckDB sources do not currently hot-reload.

## Reloading sources

After startup, Martin keeps the catalog in sync with its sources without a restart. One generic loop - the **Reload Driver** - sits behind three traits, so every source kind reuses the same loop and only the kind-specific parts vary.

```
graph LR
    Trigger["Trigger<br/>(when)"] -->|fires| Driver[Reload Driver]
    Driver -->|discover| Discovery["Discovery<br/>(what should exist)"]
    Discovery -->|"(Version, Args) per id"| Driver
    Driver -->|"diff vs baseline, build changed"| Advisory["ReloadAdvisory<br/>add / update / remove"]
    Advisory -->|apply| Sink["Sink<br/>(where)"]
    Sink --> Catalog[("Catalog<br/>TileSourceManager")]
```

The traits are:

- **`Discovery`** - the cheap **"what should exist now"** read (a directory scan or object-store listing). Returns one `(Version, Args)` per source id. `Version` is a `u128` change token (e.g. mtime millis); a changed value triggers an in-place update. `Args` is the kind-specific payload used to build the source on demand.
- **`Trigger`** - decides **when to reconcile**. `NotifyTrigger` fires on filesystem events; `PollTrigger` fires on a fixed interval.
- **`Sink`** - where the diff is **applied**. `TileSourceManager`, which holds the live catalog, implements it directly via `apply_changes`.

They interact in this loop:

1. **Init or seed**. PostgreSQL drivers `init()`: run Discovery once, apply everything, record it as the baseline before serving. `fs` sources loaded during configuration resolution, so their drivers only seed (record a baseline, apply nothing).
2. **Reconcile**. On each Trigger, re-run Discovery. Diff the new `Version`s against the baseline into a `ReloadAdvisory`. Build only the changed ids. Apply through the Sink.
3. **Commit / retain**. Advance the baseline only on a successful apply. A failed discovery or apply keeps the old baseline so the next Trigger retries the same delta, and the catalog never flaps.

Each kind has at least one Reload Driver instance. Adding reload support for a new kind means writing one `Discovery` adapter. The loop, triggers, and sink are reused.
