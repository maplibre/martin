# Configuration File

If you don't want to expose all of your tables and functions, you can list your sources in a configuration file. To
start Martin with a configuration file you need to pass a path to a file with a `--config` argument. Config files may
contain environment variables, which will be expanded before parsing. For example, to use `MY_DATABASE_URL` in your
config file: `connection_string: ${MY_DATABASE_URL}`, or with a
default `connection_string: ${MY_DATABASE_URL:-postgresql://postgres@localhost/db}`

```bash
martin --config config.yaml
```

You may wish to auto-generate a config file with `--save-config` argument. This will generate a config yaml file with
all of your configuration, which you can edit to remove any sources you don't want to expose.

```bash
martin  ... ... ...  --save-config config.yaml
```

## Config Example

```yaml
# Connection keep alive timeout [default: 75]
keep_alive: 75

# The socket address to bind [default: 0.0.0.0:3000]
listen_addresses: '0.0.0.0:3000'

# Set TileJSON URL path prefix. This overides the default of respecting the X-Rewrite-URL header.
# Only modifies the JSON (TileJSON) returned, martins' API-URLs remain unchanged. If you need to rewrite URLs, please use a reverse proxy.
# Must begin with a `/`.
# Examples: `/`, `/tiles`
base_path: /tiles

# Number of web server workers
worker_processes: 8

# Amount of memory (in MB) to use for caching tiles [default: 512, 0 to disable]
cache_size_mb: 1024

# If the client accepts multiple compression formats, and the tile source is not pre-compressed, which compression should be used. `gzip` is faster, but `brotli` is smaller, and may be faster with caching.  Default could be different depending on Martin version.
preferred_encoding: gzip

# Enable or disable Martin web UI. At the moment, only allows `enable-for-all` which enables the web UI for all connections. This may be undesirable in a production environment. [default: disable]
web_ui: disable

# Database configuration. This can also be a list of PG configs.
postgres:
  # Database connection string. You can use env vars too, for example:
  #   $DATABASE_URL
  #   ${DATABASE_URL:-postgresql://postgres@localhost/db}
  connection_string: 'postgresql://postgres@localhost:5432/db'

  # Same as PGSSLCERT for psql
  ssl_cert: './postgresql.crt'
  # Same as PGSSLKEY for psql
  ssl_key: './postgresql.key'
  # Same as PGSSLROOTCERT for psql
  ssl_root_cert: './root.crt'

  #  If a spatial table has SRID 0, then this SRID will be used as a fallback
  default_srid: 4326

  # Maximum Postgres connections pool size [default: 20]
  pool_size: 20

  # Limit the number of table geo features included in a tile. Unlimited by default.
  max_feature_count: 1000

  # Control the automatic generation of bounds for spatial tables [default: quick]
  # 'calc' - compute table geometry bounds on startup.
  # 'quick' - same as 'calc', but the calculation will be aborted if it takes more than 5 seconds.
  # 'skip' - do not compute table geometry bounds on startup.
  auto_bounds: skip

  # Enable automatic discovery of tables and functions.
  # You may set this to `false` to disable.
  auto_publish:
    # Optionally limit to just these schemas
    from_schemas:
      - public
      - my_schema
    # Here we enable both tables and functions auto discovery.
    # You can also enable just one of them by not mentioning the other,
    # or setting it to false.  Setting one to true disables the other one as well.
    # E.g. `tables: false` enables just the functions auto-discovery.
    tables:
      # Optionally set how source ID should be generated based on the table's name, schema, and geometry column
      source_id_format: 'table.{schema}.{table}.{column}'
      # Add more schemas to the ones listed above
      from_schemas: my_other_schema
      # A table column to use as the feature ID
      # If a table has no column with this name, `id_column` will not be set for that table.
      # If a list of strings is given, the first found column will be treated as a feature ID.
      id_columns: feature_id
      # Boolean to control if geometries should be clipped or encoded as is, optional, default to true
      clip_geom: true
      # Buffer distance in tile coordinate space to optionally clip geometries, optional, default to 64
      buffer: 64
      # Tile extent in tile coordinate space, optional, default to 4096
      extent: 4096
    functions:
      # Optionally set how source ID should be generated based on the function's name and schema
      source_id_format: '{schema}.{function}'

  # Associative arrays of table sources
  tables:
    table_source_id:
      # ID of the MVT layer (optional, defaults to table name)
      layer_id: table_source

      # Table schema (required)
      schema: public

      # Table name (required)
      table: table_source

      # Geometry SRID (required)
      srid: 4326

      # Geometry column name (required)
      geometry_column: geom

      # Feature id column name
      id_column: ~

      # An integer specifying the minimum zoom level
      minzoom: 0

      # An integer specifying the maximum zoom level. MUST be >= minzoom
      maxzoom: 30

      # The maximum extent of available map tiles. Bounds MUST define an area
      # covered by all zoom levels. The bounds are represented in WGS:84
      # latitude and longitude values, in the order left, bottom, right, top.
      # Values may be integers or floating point numbers.
      bounds: [ -180.0, -90.0, 180.0, 90.0 ]

      # Tile extent in tile coordinate space
      extent: 4096

      # Buffer distance in tile coordinate space to optionally clip geometries
      buffer: 64

      # Boolean to control if geometries should be clipped or encoded as is
      clip_geom: true

      # Geometry type
      geometry_type: GEOMETRY

      # List of columns, that should be encoded as tile properties (required)
      properties:
        gid: int4

  # Associative arrays of function sources
  functions:
    function_source_id:
      # Schema name (required)
      schema: public

      # Function name (required)
      function: function_zxy_query

      # An integer specifying the minimum zoom level
      minzoom: 0

      # An integer specifying the maximum zoom level. MUST be >= minzoom
      maxzoom: 30

      # The maximum extent of available map tiles. Bounds MUST define an area
      # covered by all zoom levels. The bounds are represented in WGS:84
      # latitude and longitude values, in the order left, bottom, right, top.
      # Values may be integers or floating point numbers.
      bounds: [ -180.0, -90.0, 180.0, 90.0 ]

# Publish PMTiles files from local disk or proxy to a web server
pmtiles:
  paths:
    # scan this whole dir, matching all *.pmtiles files
    - /dir-path
    # specific pmtiles file will be published as a pmt source (filename without extension)
    - /path/to/pmt.pmtiles
    # A web server with a PMTiles file that supports range requests
    - https://example.org/path/tiles.pmtiles
  sources:
    # named source matching source name to a single file
    pm-src1: /path/to/pmt.pmtiles
    # A named source to a web server with a PMTiles file that supports range requests
    pm-web2: https://example.org/path/tiles.pmtiles

# Publish MBTiles files
mbtiles:
  paths:
    # scan this whole dir, matching all *.mbtiles files
    - /dir-path
    # specific mbtiles file will be published as mbtiles2 source
    - /path/to/mbtiles.mbtiles
  sources:
    # named source matching source name to a single file
    mb-src1: /path/to/mbtiles1.mbtiles

# Sprite configuration
sprites:
  paths:
    # all SVG files in this dir will be published as a "my_images" sprite source
    - /path/to/my_images
  sources:
    # SVG images in this directory will be published as a "my_sprites" sprite source
    my_sprites: /path/to/some_dir
  make_sdf: false

# Font configuration
fonts:
  # A list of *.otf, *.ttf, and *.ttc font files and dirs to search recursively.
  - /path/to/font/file.ttf
  - /path/to/font_dir
```
