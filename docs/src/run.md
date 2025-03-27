# Usage

Martin requires at least one PostgreSQL [connection string](pg-connections.md) or a [tile source file](sources-files.md)
as a command-line argument. A PG connection string can also be passed via the `DATABASE_URL` environment variable.

```bash
martin postgresql://postgres@localhost/db
```

Martin provides [TileJSON](https://github.com/mapbox/tilejson-spec) endpoint for
each [geospatial-enabled](https://postgis.net/docs/using_postgis_dbmanagement.html#geometry_columns) table in your
database.
