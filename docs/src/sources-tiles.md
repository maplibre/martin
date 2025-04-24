# Tile Sources

Martin supports multiple tile sources

- [MBTiles Sources](sources-files.md) Local Sqlite database containing pre-generated vector or raster tiles.
- [PMTiles Sources](sources-files.md) A local file or a web-accessible HTTP source with the pre-generated raster or vector tiles.
- [PostgreSQL Connections](pg-connections.md) with
  - [Table Sources](sources-pg-tables.md)
  - [Function Sources](sources-pg-functions.md)

The difference between tile archives (*[MBTiles/PMTiles](sources-files.md)*) and a database ([PG-Table](sources-pg-tables.md)/[PG-Function](sources-pg-functions.md)) is that

- **database** are more flexible and may (depending on how you fill it) be updated in **real-time**.
- **Tile archives** on the other hand may (depending on the data) be more **compact, memory efficient and exhibit better performance** for tile-serving.

=> For most usecases, you may want a mix of both. We support this via [Composite Sources](sources-composite.md)
=> For some usecases, you want the flexibility of a database, but you don't want to pay the runtime-price. We offer the [`martin-cp`](martin-cp.md) utility to render all tiles into a tile archive. This can also be used to provide offline maps via [diffing and syncing `mbtiles`](mbtiles-diff.md)

The difference between MBTiles and PMTiles is that:

- **MBTiles** require the entire archive to be on the same machine. **PMTiles** can utilise a remote HTTP-Range request supporting server or a local file.
- Performance wise, **MBTiles** is slightly faster than **PMTiles**, but with caching this is negligible.
- Disk size wise, **MBTiles** is slightly (10-15%) higher than **PMTiles**.
- **PMTiles** requires less memory in extreme cases as sqlite has a small in-memory cache.

The choice depends on your specific usecase and requirements.
