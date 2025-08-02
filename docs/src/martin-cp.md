# Generating Tiles in Bulk

We offer the `martin-cp` tool for generating tiles in bulk, from any source(s) supported by Martin, and save retrieved tiles into a new or an existing MBTiles file.

`martin-cp` can be used to generate tiles for a large area or multiple areas (bounding boxes).
If multiple areas overlap, it will ensure each tile is generated only once
`martin-cp` supports the same configuration file and CLI arguments as Martin server, so it can support all sources and even combining sources.

After copying, `martin-cp` will update the `agg_tiles_hash` metadata value unless `--skip-agg-tiles-hash` is specified.
This allows the MBTiles file to be [validated](mbtiles-validation.md#aggregate-content-validation) using `mbtiles validate` command.

## Usage

This copies tiles from a PostGIS table `my_table` into an MBTiles file `tileset.mbtiles` using [normalized](mbtiles-schema.md#normalized) schema, with zoom levels from 0 to 10, and bounds of the whole world.

```bash
martin-cp  --output-file tileset.mbtiles \
           --mbtiles-type normalized     \
           "--bbox=-180,-90,180,90"      \
           --min-zoom 0                  \
           --max-zoom 10                 \
           --source source_name          \
           postgresql://postgres@localhost:5432/db
```

> [!TIP]
> `--concurrency <CONCURRENCY>` and `--pool-size <POOL_SIZE>` can be used to control the number of concurrent requests and the pool size for postgres sources respectively.
>
> The optimal setting depends on:
>
> - the source(s) performance characteristics
> - how much load is allowed (f.ex. multi-tenant environment)
> - how the tile is stored in the file should be compressed

You should also consider

> [!TIP]
> `--encoding <ENCODING>` can be used to reduce the final size of the MBTiles file or decrease the amount of processing `martin-cp` does.
>
> Our default (`gzip`) should be a reasonable choice for most use cases, but if you prefer a different encoding, you can specify it here.
> If set to multiple values like gzip,br, martin-cp will use the first encoding, or re-encode if the tile is already encoded and that encoding is not listed.
> Use `identity` to disable compression.
> Ignored for non-encodable tiles like PNG and JPEG.

## Arguments

```raw
martin-cp --help
A tool to bulk copy tiles from any Martin-supported sources into an mbtiles file

Usage: martin-cp [OPTIONS] --output-file <OUTPUT_FILE> [CONNECTION]...

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

          [possible values: flat, flat-with-hash, normalized]

      --url-query <URL_QUERY>
          Optional query parameter (in URL query format) for the sources that support it (e.g. Postgres functions)

      --encoding <ENCODING>
          Optional accepted encoding parameter as if the browser sent it in the HTTP request.

          If set to multiple values like gzip,br, martin-cp will use the first encoding, or re-encode if the tile is already encoded and that encoding is not listed. Use identity to disable compression. Ignored for non-encodable tiles
          like PNG and JPEG.

          [default: gzip]

      --on-duplicate <ON_DUPLICATE>
          Allow copying to existing files, and indicate what to do if a tile with the same Z/X/Y already exists

          [possible values: override, ignore, abort]

      --concurrency <CONCURRENCY>
          Number of concurrent connections to use

          [default: 1]

      --bbox <BBOX>
          Bounds to copy, in the format min_lon,min_lat,max_lon,max_lat. Can be specified multiple times. Overlapping regions will be handled correctly

      --min-zoom <MIN_ZOOM>
          Minimum zoom level to copy

      --max-zoom <MAX_ZOOM>
          Maximum zoom level to copy

  -z, --zoom-levels <ZOOM_LEVELS>
          List of zoom levels to copy

      --skip-agg-tiles-hash
          Skip generating a global hash for mbtiles validation. By default, martin-cp will compute and update agg_tiles_hash metadata value

      --set-meta <KEY=VALUE>
          Set additional metadata values. Must be set as "key=value" pairs. Can be specified multiple times

  -c, --config <CONFIG>
          Path to config file. If set, no tile source-related parameters are allowed

      --save-config <SAVE_CONFIG>
          Save resulting config to a file or use "-" to print to stdout. By default, only print if sources are auto-detected

  -C, --cache-size <CACHE_SIZE>
          Main cache size (in MB)

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

  -m, --max-feature-count <MAX_FEATURE_COUNT>
          Limit the number of geo features per tile.

          If the source table has more features than set here, they will not be included in the tile and the result will look "cut off"/incomplete. This feature allows to put a maximum latency bound on tiles with extreme amount of
          detail at the cost of not returning all data. It is sensible to set this limit if you have user generated/untrusted geodata, e.g. a lot of data points at Null Island.

          Can be either a positive integer or unlimited if omitted.

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin_cp=debug. See https://docs.rs/env_logger/latest/env_logger/index.html#enabling-logging for more information.
```
