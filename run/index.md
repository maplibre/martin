# Usage

Martin requires at least one PostgreSQL [connection string](<https://maplibre.org/martin/pg-connections/index.md>) or a [tile source file](<https://maplibre.org/martin/sources-files/index.md>) as a command-line argument. Sources can also be listed in a [configuration file](<https://maplibre.org/martin/config-file/index.md>).

[DuckDB / GeoParquet sources](<https://maplibre.org/martin/sources-duckdb/index.md>) require Martin to be built with `--features=unstable-duckdb`. Pass a local file with `martin data.parquet` or `martin tiles.duckdb` to use the default settings. Use a configuration file for remote GeoParquet or custom DuckDB settings.

```bash
martin postgres://postgres@localhost/db
```

Martin provides a [TileJSON](<https://github.com/mapbox/tilejson-spec>) endpoint for each [geospatial-enabled](<https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns>) table in your database.
