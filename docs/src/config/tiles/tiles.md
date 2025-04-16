# Tile Sources

Martin does support these tile sources:

- [MBTiles Sources](sources-files.md) Local Sqlite database containing pre-rendered tiles.
- [PMTiles Sources](sources-files.md) Local or remote file with pre-rendered tiles in a pre-defined order.
- [PostgreSQL Connections](pg-connections.md) via
  - [Table Sources](pg-tables.md)
  - [Function Sources](pg-functions.md)

The difference between tile archives (*[MBTiles/PMTiles](sources-files.md)*) and a database ([PG-Table](pg-tables.md)/[PG-Function](pg-functions.md)) is that
- **database** are more flexible and may (depending on how you fill it) be updated in **real-time**.
- **Tile archives** on the other hand may (depending on the data) be more **compact, memory efficient and exhibit better performance** for tile-serving.

=> For most usecases, you may want a mix of both. We support this via [Composite Sources](sources-composite.md)

The difference between MBTiles and PMTiles is that
- **MBTiles** require the entire archive to be on the same machine. **PMTiles** can utilise a remote HTTP-Range request supporting server or a local file.
- Performance wise, **MBTiles** is slightly higher than **PMTiles**, but with caching this is negligible.
- Disk size wise, **MBTiles** is slightly (10-15%) higher than **PMTiles**.
- **PMTiles** requires less memory in extreme cases as sqlite has a small in-memory cache.

Given that these are tradeoffs, there is no clear winner. The choice depends on your specific usecase and requirements.
