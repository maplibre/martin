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

!!! warning "Not released, not fully implemented"
    `martin generate` is not in any release yet, and parts of what these pages describe are not
    implemented yet. It is only built with the `unstable-generate` feature:

    ```bash
    cargo build --package martin --bin martin --features unstable-generate
    ```

`martin generate` builds a whole tileset from PostgreSQL tables and views in one pass.
Unlike [`martin cp`](martin-cp.md), which asks the source for every tile (one `ST_AsMVT` query per tile),
it reads each table once, renders every feature at every zoom itself, and writes the tiles in order.
For large tilesets this is much faster and puts far less load on the database.
It renders points, lines and polygons. Geometry collections are skipped and counted.

## Usage

This renders the table sources `roads` and `shops` into `tileset.mbtiles`, zooms 0 to 14:

```bash
martin generate --output-file tileset.mbtiles \
                --source roads                \
                --source shops                \
                --max-zoom 14                 \
                postgres://postgres@localhost:5432/db
```

Without `--source`, every table source is rendered.
Sources come from the same connection strings or [configuration file](config-file/index.md) as for the
tile server. Function sources cannot be rendered, and all sources must come from one PostgreSQL connection.

Each table source becomes one layer, named by its `layer_id` or its source id. A table source with
[`layers`](sources-pg-layers.md) instead feeds several layers from one read of the table, each with
its own features, zooms, attributes and draw order.

The same data and options produce the same tiles, whatever the number of threads.

## Configuration file

The configuration file only describes the sources. The options of a run are command-line arguments,
and the file has no section for `martin generate`.

```bash
martin generate --config config.yaml --output-file tileset.pmtiles --max-zoom 14
```

```yaml
postgres:
  connection_string: postgres://postgres@localhost:5432/db
  tables:
    shops:
      schema: public
      table: shops
      geometry_column: geom
      id_column: osm_id
      minzoom: 12
      properties:
        name: text
        shop: text
    roads:
      schema: public
      table: roads
      geometry_column: geom
      properties:
        class: text
        name: text
      layers:
        roads:
          attributes: [class]
        road_labels:
          minzoom: 10
          where: "name != null"
          attributes: [name]
```

Of a [table source](sources-pg-tables.md), `martin generate` uses `schema`, `table`, `geometry_column`,
`srid`, `id_column`, `properties`, `filter`, `minzoom`, `maxzoom`, `extent`, `buffer`,
`layer_id` and [`layers`](sources-pg-layers.md). Settings that only affect serving, such as caching, are
ignored.

## Options

| Option          | Default         | Meaning                                                              |
|-----------------|-----------------|----------------------------------------------------------------------|
| `--output-file` |                 | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty |
| `--source`      | all tables      | A table source id, repeatable                                        |
| `--format`      | `mlt`           | `mlt` or `mvt`                                                       |
| `--encoding`    | `gzip`          | `gzip`, `zstd`, `br`, `zlib` (MBTiles only) or `none`                |
| `--min-zoom`    | `0`             | Lowest zoom, raised by a source's `minzoom`                          |
| `--max-zoom`    | `14`            | Highest zoom, lowered by a source's `maxzoom`                        |
| `--bbox`        | whole world     | `min_lon,min_lat,max_lon,max_lat`: only features and tiles inside it |
| `--memory`      | `2GiB`          | Memory for sorting, split over the threads                           |
| `--temp-dir`    | system temp dir | Where sorted runs go, repeatable to spread them over several disks   |
| `--threads`     | CPU count       | Worker threads                                                       |

The [configuration and PostgreSQL arguments](run-with-cli.md) of the tile server also apply, such as
`--config` and `--pool-size`. A configuration file and connection strings cannot be combined.

Unlike `martin cp`, `--bbox` filters features: a low-zoom tile at the edge of the box only holds the
features near the box, not everything the tile covers.

The highest zoom is 27, and the tile grid limits it further: `extent × 2^zoom` must not exceed `2^30`,
i.e. zoom 18 for the default extent of 4096.

A failed or interrupted run removes the output file. Temporary files are always removed.

## Sizing a run

- **`--memory`** is split evenly over the render workers, each getting between 16 MiB and 4 GiB, so with
  many threads the total can exceed `--memory`. More memory means fewer, longer sorted runs. Up to 256
  runs are merged at once. With more, groups of runs are first merged into longer ones, which reads
  and writes the temporary data once more.
- **Disk:** the temporary data holds a piece of every feature for every tile it touches at every zoom,
  so it grows with the zoom range and is usually several times the size of the input. A run with more
  than 256 sorted runs needs room for one more copy while it merges them.
- **`--temp-dir`** takes the sorted runs round-robin. Several directories on separate disks spread the I/O.
- **`--threads`** sets the render workers and the encoders. Tables are read over up to one database
  connection per thread, limited by the connection pool's `pool_size`.

[Generation Architecture](architecture-generate.md) explains how a run reads, sorts, encodes
and writes the tiles, and what bounds its memory.
