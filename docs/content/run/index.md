---
icon: material/play-circle
---

# Usage

Martin requires at least one PostgreSQL [connection string](../pg-connections/index.md) or a [tile source file](../sources-files/index.md)
as a command-line argument.
A PG connection string can also be passed via the `DATABASE_URL` environment variable.
[DuckDB / GeoParquet sources](../sources-duckdb.md) are configured in a [configuration file](../config-file/index.md) instead, and require Martin to be built with `--features=unstable-duckdb`.

```bash
martin postgres://postgres@localhost/db
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for
each [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) table in your
database.
