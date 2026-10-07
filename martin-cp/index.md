# Generating Tiles in Bulk

We offer the `martin cp` subcommand for generating tiles in bulk, from any source(s) supported by Martin, and save retrieved tiles into a new or an existing MBTiles file.

`martin cp` can be used to generate tiles for a large area or multiple areas (bounding boxes). If multiple areas overlap, it will ensure each tile is generated only once `martin cp` supports the same configuration file and CLI arguments as the Martin server, so it can support all sources and even combining sources. The released `martin` binary does not include [DuckDB / GeoParquet sources](<https://maplibre.org/martin/sources-duckdb/index.md>). To use them, build `martin` with the feature and list the source in the configuration file:

```bash
cargo build --package martin --bin martin --features=unstable-duckdb
```

After copying, `martin cp` will update the `agg_tiles_hash` metadata value unless `--skip-agg-tiles-hash` is specified. This allows the MBTiles file to be [validated](<https://maplibre.org/martin/mbtiles-validation/#aggregate-content-validation>) using `mbtiles validate` command.

## Usage

This copies tiles from a PostGIS table `my_table` into an MBTiles file `tileset.mbtiles` using [normalized](<https://maplibre.org/martin/mbtiles-schema/#normalized>) schema, with zoom levels from 0 to 10, and xyz-compliant tile bounds of the whole world.

```bash
martin cp  --output-file tileset.mbtiles                         \
           --mbtiles-type normalized                             \
           "--bbox=-180,-85.05112877980659,180,85.0511287798066" \
           --min-zoom 0                                          \
           --max-zoom 10                                         \
           --source source_name                                  \
           postgres://postgres@localhost:5432/db
```

> [!TIP]
>
> Next to regular sources, `--source <SOURCE>` does support [composite sources](<https://maplibre.org/martin/sources-composite/index.md>). This means `martin cp` can be used to merge two different sources into one `mbtiles` archive.

If performance is a concern, you should also consider

> [!TIP]
>
> `--concurrency <CONCURRENCY>` and `--pool-size <POOL_SIZE>` can be used to control the number of concurrent requests and the pool size for postgres sources respectively.
>
> The optimal setting depends on:
>
> - the source(s) performance characteristics
> - how much load is allowed, for example in a multi-tenant environment
> - how to compress tiles stored in the output file

You should also consider

> [!TIP]
>
> `--encoding <ENCODING>` can be used to reduce the final size of the MBTiles file or decrease the amount of processing `martin cp` does.
>
> The default `gzip` should be a reasonable choice for most use cases, but if you prefer a different encoding, you can specify it here. If set to multiple values like `'gzip,br'`, `martin cp` will use the first encoding, or re-encode if the tile is already encoded and that encoding is not listed. Use `identity` to disable compression. Ignored for non-encodable tiles like PNG and JPEG.

> [!NOTE]
>
> When the source (such as PG tables) guarantees that an empty tile only has empty tiles below it, `martin cp` copies zoom by zoom and never fetches the tiles below an empty tile. This means that even sparse sources can usually be fairly performant.

## MLT from PostgreSQL tables

With `--format mlt`, `martin cp` encodes a PostgreSQL table source's tiles straight from its rows instead of converting the MVT tile PostGIS builds. The tiles describe the same features as that conversion would:

- A column type a tile property cannot hold, such as an array, a `numeric` or a timestamp, is written as its text, as `ST_AsMVT` writes it.
- A `jsonb` column is spread over one property per top-level key that holds a string, a boolean or a number, as `ST_AsMVT` spreads it. Nested objects, arrays and `null`s are left out.

Function sources, and tiles the row query cannot encode, go through the MVT conversion instead, and `martin cp` logs a warning when that happens.

A `martin` built with the `unstable-mlt-v2` feature also accepts `--format mlt2`, which keeps what the v1 wire format has no place for:

- A `jsonb` document is kept whole, nested values included, in a nested column named after the `jsonb` column.
- The M ordinates of measured geometries are kept in a vertex column named `m`. PostGIS drops the M ordinates of every geometry it clips at the tile border, so with the default `clip_geom: true` only the features that lie wholly inside the tile and its buffer keep them. Set `clip_geom: false` on the table to keep them everywhere.

## Arguments

Use `martin cp --help` to see a list of available options:

```text
Bulk copy tiles from any Martin-supported sources into an mbtiles file

Usage: martin cp [OPTIONS] --output-file <OUTPUT_FILE> [CONNECTION]...

Arguments:
  [CONNECTION]...
          Connection strings, e.g. postgres://... or /path/to/files

Options:
  -s, --source <SOURCE>
          Name of the source to copy from. Not required if there is only one source

  -o, --output-file <OUTPUT_FILE>
          Path to the mbtiles file to copy to

      --mbtiles-type <SCHEMA>
          Output format of the new destination file. Ignored if the file exists. [DEFAULT: normalized]

          [possible values: flat, flat-with-hash, normalized, cache]

      --url-query <URL_QUERY>
          Optional query parameter (in URL query format) for the sources that support it (e.g. Postgres functions)

      --encoding <ENCODING>
          Optional accepted encoding parameter as if the browser sent it in the HTTP request.

          If set to multiple values like gzip,br, the first encoding is used, or re-encode if the tile is already encoded and that encoding is not listed. Use identity to disable compression. Ignored for non-encodable tiles like PNG and JPEG.

          [default: gzip]

      --format <FORMAT>
          Tile format to request from the source.

          A vector source converts between `mvt` and `mlt` unless `convert_to_{mlt,mvt}` turns that off, and a source that can neither produce nor convert to this format fails the copy. Defaults to what the source produces.

          Possible values:
          - mvt:  Mapbox Vector Tile
          - mlt1: MapLibre Tile, v1 wire format

      --on-duplicate <ON_DUPLICATE>
          Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists

          [possible values: override, ignore, abort]

      --concurrency <CONCURRENCY>
          Number of concurrent connections to use

          [default: 1]

      --bbox <BBOX>
          Bounds to copy, in the format min_lon,min_lat,max_lon,max_lat. Can be specified multiple times with overlapping bounds being handled correctly. Maximum bounds follows mbtiles specification for xyz-compliant tile bounds.

          If omitted, will first default to configured source bounds if present. Otherwise, will default to global xyz-compliant tile bounds.

          For a source on a tile grid other than Web Mercator the bounds are min_x,min_y,max_x,max_y in the grid's CRS units, and the whole grid is copied if omitted.

      --min-zoom <MIN_ZOOM>
          Minimum zoom level to copy

      --max-zoom <MAX_ZOOM>
          Maximum zoom level to copy

  -z, --zoom-levels <ZOOM_LEVELS>
          List of zoom levels to copy

      --skip-agg-tiles-hash
          Skip generating a global hash for mbtiles validation. By default, the agg_tiles_hash metadata value is computed and updated

      --set-meta <KEY=VALUE>
          Set additional metadata values. Must be set as "key=value" pairs. Can be specified multiple times

  -c, --config <CONFIG>
          Path to config file. If set, no tile source-related parameters are allowed

      --save-config <SAVE_CONFIG>
          Save resulting config to a file or use "-" to print to stdout. By default, only print if sources are auto-detected

      --on-invalid <ON_INVALID>
          Action to take when a source is found to be invalid during startup. [DEFAULT: abort]

          Possible values:
          - warn:  Log warning messages, abort if the error is critical
          - abort: Log warnings as errors, abort startup

      --no-tui
          Print the log stream instead of the live dashboard an interactive terminal gets by default

  -b, --auto-bounds <AUTO_BOUNDS>
          Specify how bounds should be computed for the spatial PG tables. [DEFAULT: quick]

          Possible values:
          - quick: Compute table geometry bounds, but abort if it takes longer than 5 seconds
          - calc:  Compute table geometry bounds. The startup time may be significant. Make sure all GEO columns have indexes
          - skip:  Skip bounds calculation. The bounds will be set to the whole world

      --ca-root-file <CA_ROOT_FILE>
          Loads trusted root certificates from a file. The file should contain a sequence of PEM-formatted CA certificates

  -d, --default-srid <DEFAULT_SRID>
          If a spatial PG table has SRID 0, then this default SRID will be used as a fallback

  -p, --pool-size <POOL_SIZE>
          Maximum Postgres connections pool size [DEFAULT: 20]

      --pg-retry-timeout <PG_RETRY_TIMEOUT>
          How long the first PostgreSQL connection is retried before startup fails, a duration like 30s or infinite. [DEFAULT: 30s]

  -m, --max-feature-count <MAX_FEATURE_COUNT>
          Limit the number of geo features per tile.

          If the source table has more features than set here, they will not be included in the tile and the result will look "cut off"/incomplete. This feature allows to put a maximum latency bound on tiles with extreme amount of detail at the cost
          of not returning all data. It is sensible to set this limit if you have user generated/untrusted geodata, e.g. a lot of data points at Null Island.

          Can be either a positive integer or unlimited if omitted.

      --ssl-cert <SSL_CERT>
          A file with a client SSL certificate

      --ssl-key <SSL_KEY>
          A file with the key for the client SSL certificate

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin=debug.
Use RUST_LOG_FORMAT environment variable to control output format: json, full, compact (default), bare or pretty. With RUST_LOG_FORMAT=json, configuration error diagnostics are also emitted as structured JSON for editor tooling and log aggregation.
See https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html for more information.
```
