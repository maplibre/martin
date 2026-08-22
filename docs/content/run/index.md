---
icon: material/play-circle
---

# Usage

Martin requires at least one PostgreSQL [connection string](../pg-connections/index.md) or a [tile source file](../sources-files/index.md)
as a command-line argument.
A PG connection string can also be set in a [configuration file](../config-file/index.md), where it may reference an environment variable, e.g. `connection_string: ${DATABASE_URL}`.
Martin does not read `DATABASE_URL` on its own -- see [environment variables](../env-vars.md).
[DuckDB / GeoParquet sources](../sources-duckdb.md) are configured in a [configuration file](../config-file/index.md) instead, and require Martin to be built with `--features=unstable-duckdb`.

```bash
martin postgres://postgres@localhost/db
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for
each [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) table in your
database.
