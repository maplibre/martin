## Command-line Interface

You can configure Martin using command-line interface.
See `martin --help` or `cargo run -- --help` for more information:

```text
Blazing fast and lightweight tile server with PostGIS, MBTiles, and PMTiles support

Usage: martin [OPTIONS] [CONNECTION]...

Arguments:
  [CONNECTION]...
          Connection strings, e.g. postgres://... or /path/to/files

Options:
  -c, --config <CONFIG>
          Path to config file. If set, no tile source-related parameters are allowed

      --save-config <SAVE_CONFIG>
          Save resulting config to a file or use "-" to print to stdout.
          By default, only print if sources are auto-detected

  -C, --cache-size <CACHE_SIZE>
          Main cache size (in MB)

  -s, --sprite <SPRITE>
          Export a directory with SVG files as a sprite source. Can be specified multiple times

  -f, --font <FONT>
          Export a font file or a directory with font files as a font source (recursive). Can be specified multiple times

  -k, --keep-alive <KEEP_ALIVE>
          Connection keep alive timeout. [DEFAULT: 75]

  -l, --listen-addresses <LISTEN_ADDRESSES>
          The socket address to bind. [DEFAULT: 0.0.0.0:3000]

      --base-path <BASE_PATH>
          Set TileJSON URL path prefix.

          This overrides the default of respecting the X-Rewrite-URL header.
          Only modifies the JSON (TileJSON) returned, martins' API-URLs remain unchanged.
          If you need to rewrite URLs, please use a reverse proxy.
          Must begin with a /.

          Examples: /, /tiles

  -W, --workers <WORKERS>
          Number of web server workers

      --preferred-encoding <PREFERRED_ENCODING>
          Martin server preferred tile encoding. [DEFAULT: gzip]

          If the client accepts multiple compression formats, and the tile source is not pre-compressed, which compression should be used. gzip is faster, but brotli is smaller, and may be faster with caching.

          [possible values: brotli, gzip]

  -u, --webui <WEB_UI>
          Control Martin web UI. [DEFAULT: disabled]

          Possible values:
          - disable:        Disable Web UI interface. This is the default, but once implemented, the default will be enabled for localhost.
          - enable-for-all: Enable Web UI interface on all connections

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
          Limit the number of features in a tile from a PG table source

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

Use RUST_LOG environment variable to control logging level, e.g. RUST_LOG=debug or RUST_LOG=martin=debug. See https://docs.rs/env_logger/latest/env_logger/index.html#enabling-logging for more information.
```
