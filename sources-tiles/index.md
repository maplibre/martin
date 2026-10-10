# Tile Sources

Martin supports multiple tile sources

- [Tile Archive files](<https://maplibre.org/martin/sources-files/index.md>)
- [MBTiles Sources](<https://maplibre.org/martin/sources-mbtiles/index.md>) Local Sqlite database containing pre-generated vector or raster tiles.
- [PMTiles Sources](<https://maplibre.org/martin/sources-pmtiles/index.md>) A local file or a web-accessible HTTP source with the pre-generated raster or vector tiles.
- [GeoJSON Sources](<https://maplibre.org/martin/sources-geojson/index.md>) A local file with geodata that we can convert to vector tiles.
- [DuckDB Sources](<https://maplibre.org/martin/sources-duckdb/index.md>) GeoParquet files that we can convert to vector tiles (unstable).
- [PostgreSQL Connections](<https://maplibre.org/martin/pg-connections/index.md>) with
- [Table Sources](<https://maplibre.org/martin/sources-pg-tables/index.md>)
- [Function Sources](<https://maplibre.org/martin/sources-pg-functions/index.md>)

The difference between tile archives (*[MBTiles/PMTiles](<https://maplibre.org/martin/sources-files/index.md>)*), semi-static data (*[GeoJSON](<https://maplibre.org/martin/sources-geojson/index.md>)* / *[DuckDB](<https://maplibre.org/martin/sources-duckdb/index.md>)*) and a database ([PG-Table](<https://maplibre.org/martin/sources-pg-tables/index.md>)/[PG-Function](<https://maplibre.org/martin/sources-pg-functions/index.md>)) is that

- **database** are more flexible and may (depending on how you fill it) be updated in **real-time**.
- **Tile archives** on the other hand may (depending on the data) be more **compact, memory efficient and exhibit better performance** for tile-serving.
- **semi-static data** is for the use case when the **data is relatively static** and **not large** enough to justify converting it to a tile archive

> [!TIP]
>
> For most use cases, you may want a mix of both. We support this via [Composite Sources](<https://maplibre.org/martin/sources-composite/index.md>) For some use cases, you want the flexibility of a database, but you don't want to pay the runtime-price. We offer the [`martin cp`](<https://maplibre.org/martin/martin-cp/index.md>) subcommand to render all tiles into a tile archive. This can also be used to provide offline maps via [diffing and syncing `mbtiles`](<https://maplibre.org/martin/mbtiles-diff/index.md>)

The difference between MBTiles and PMTiles is that:

- **MBTiles** require the entire archive to be on the same machine. **PMTiles** can utilize a remote HTTP-Range request supporting server or a local file.
- Performance wise, **MBTiles** is slightly faster than **PMTiles**, but with caching this is negligible.
- Disk size wise, **MBTiles** is slightly (10-15%) higher than **PMTiles**.
- **PMTiles** requires less memory in extreme cases as sqlite has a small in-memory cache.

The choice depends on your specific use case and requirements.

Most vector tile sources support optional [postprocessing](<https://maplibre.org/martin/postprocessing/index.md>) (format conversion) via the `convert_to_mlt` and `convert_to_mvt` configuration keys. DuckDB / GeoParquet sources do not currently support postprocessing.

Tile sources also accept a per-source `cache_control` configuration key that sets the `Cache-Control` response header for that source's tiles, overriding the top-level `cache_control` default. A composite tile request uses the per-source value only when every requested source is configured with the same one; otherwise the response falls back to the top-level default.

> [!TIP]
>
> **`cache_control` handling**
>
> When no `cache_control` is configured, PMTiles and MBTiles tiles set `max-age=0, stale-while-revalidate=86400`. Therefore, a returning visitor's browser draws tiles it has cached right away and checks them with the server in the background. A server side rewritten archive may only show up on the visitor's next load due to invalidation if the tiles are cached on the client and the `stale-while-revalidate`-check in the background suggests that the tiles martin offers are stale. Set `cache_control: no-cache` if browsers should check every tile before drawing it.
