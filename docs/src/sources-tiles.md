# Tile Sources

Martin supports multiple tile sources

- Static tile archives
  - [MBTiles Sources](sources-files.md) Local Sqlite database containing pre-generated vector or raster tiles.
  - [PMTiles Sources](sources-files.md) A local file or a web-accessible HTTP source with the pre-generated raster or vector tiles.
- [GeoJSON Sorces](sources-geojson.md) A local file with geodata that we can convert to vector tiles.
- [PostgreSQL Connections](pg-connections.md) with
  - [Table Sources](sources-pg-tables.md)
  - [Function Sources](sources-pg-functions.md)

The difference between tile archives (*[MBTiles/PMTiles](sources-files.md)*), semi-static data (*[GeoJSON](sources-geojson.md)*) and a database (*[PG-Table](sources-pg-tables.md)/[PG-Function](sources-pg-functions.md)*) is that

- **database** are more flexible and may (depending on how you fill it) be updated in **real-time**.
- **Tile archives** on the other hand may (depending on the data) be more **compact, memory efficient and exhibit better performance** for tile-serving.
- semi-static data is reserved for the usecase when the data is relatively static and not large enough to justify converting it to a tile archive

> []
> For most usecases, you may want a mix. We support this via [Composite Sources](sources-composite.md)

> [!TIP]
> For some usecases, you want the flexibility of a database, but you don't want to pay the runtime-price.
> We offer the [`martin-cp`](martin-cp.md) utility to render all tiles into a tile archive.
> This can also be used to provide offline maps via [diffing and syncing `mbtiles`](mbtiles-diff.md)
