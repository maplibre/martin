---
description: Generating a whole tileset from PostgreSQL tables in one pass
icon: material/layers-triple
tags:
  - tooling
  - mbtiles
  - pmtiles
  - postgres
---

# Generating Tilesets from Tables

`martin generate` builds a whole tileset from PostgreSQL tables and views in one pass.
Unlike [`martin cp`](martin-cp.md), which asks the source for every tile (one `ST_AsMVT` query per tile),
it reads each table once, renders every feature at every zoom itself, and writes the tiles in order.
For large tilesets this is much faster and puts far less load on the database.

!!! warning
    `martin generate` is new. It renders points, lines and polygons; geometry collections are
    skipped and counted. The feature is not in the default build:

    ```bash
    cargo build --package martin --bin martin --features unstable-generate
    ```

## Usage

This renders the table sources `roads` and `shops` as two layers of `tileset.mbtiles`, zooms 0 to 14:

```bash
martin generate --output-file tileset.mbtiles \
                --source roads                \
                --source shops=places         \
                --max-zoom 14                 \
                postgres://postgres@localhost:5432/db
```

Every table source becomes one layer. Its name comes from the source's `layer_id`, or from the
source id, and `--source id=name` renames it. Without `--source`, every table source is rendered.
Sources are configured exactly as for the tile server, with the same connection arguments and
[configuration file](config-file/index.md): `extent`, `buffer`, `minzoom`, `maxzoom`, `properties`,
`id_column`, `filter` and `clip_geom` of each table source apply. Function sources cannot be scanned.

## How it works

1. Each table is split into partitions that are scanned in parallel: page (`ctid`) ranges for tables
   and materialized views on PostgreSQL 14 and newer, and id ranges for views with an integer
   `id_column`. Anything else is read as one stream.
2. Each feature is rendered for every zoom of its layer: features smaller than a pixel are dropped,
   lines and polygon rings are simplified, and the geometry is sliced into the tiles it touches,
   keeping its own vertices (no new vertices are cut at tile edges; a polygon ring leaving a tile is
   closed by corners just outside the tile's buffer, where renderers clip it away). Tiles a polygon
   covers entirely are recorded as ranges, not one by one, so huge polygons cost time and space in
   proportion to their outline.
3. The pieces are sorted on disk by tile, merged, encoded as MLT or MVT, compressed, and written in
   the order the output stores them: MBTiles key order, or the Hilbert order of a clustered PMTiles
   archive. Identical tiles are stored once.

The output is deterministic: the same data and options produce the same file, whatever the number of
threads. Features keep their source order within a tile, so a view's `ORDER BY` controls the draw
order. Partitioned views are read in id order.

## Options

| Option              | Default         | Meaning                                                                 |
|---------------------|-----------------|-------------------------------------------------------------------------|
| `--output-file`     |                 | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty    |
| `--source`          | all tables      | `id` or `id=layer_name`, repeatable                                     |
| `--format`          | `mlt`           | `mlt` or `mvt`                                                          |
| `--encoding`        | `gzip`          | `gzip`, `zstd`, `br`, `zlib` (MBTiles only) or `none`                   |
| `--min-zoom`        | `0`             | Lowest zoom; a source's `minzoom` raises it                             |
| `--max-zoom`        | `14`            | Highest zoom; a source's `maxzoom` lowers it                            |
| `--bbox`            | whole world     | `min_lon,min_lat,max_lon,max_lat`: only features and tiles inside it    |
| `--memory`          | `2GiB`          | Memory for sorting, split over the threads                              |
| `--temp-dir`        | system temp dir | Where sorted runs go; repeat it to spread them over several disks       |
| `--threads`         | CPU count       | Worker threads                                                          |

Unlike `martin cp`, `--bbox` filters features: a low-zoom tile at the edge of the box only holds the
features inside the box, not everything the tile covers.

Temporary files need roughly 5 to 10 times the size of the input data, and are removed automatically,
also when the run fails or is interrupted. A failed or interrupted run removes the partial output.

The highest zoom is limited by the tile grid: `extent × 2^zoom` must not exceed `2^30`, i.e. zoom 18 for
the default extent of 4096.
