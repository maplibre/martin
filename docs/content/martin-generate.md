---
description: Generating a whole tileset in one pass
icon: material/layers-triple
tags:
  - tooling
  - mbtiles
  - pmtiles
  - postgres
---

# Generating Tilesets

--8<-- "generate-unstable.md"

`martin generate` builds a whole tileset in one pass. [`martin cp`](martin-cp.md) asks the source for
each tile separately. `martin generate` instead reads each source once, renders every feature at every
zoom itself, and writes the tiles in order. For large tilesets this is much faster, and it puts far less
load on the source.

It renders points, lines and polygons. Geometry collections are skipped and counted.

## Usage

```bash
martin generate --output-file tileset.mbtiles \
                --source roads                \
                --source shops                \
                --max-zoom 14                 \
                postgres://postgres@localhost:5432/db
```

This renders the sources `roads` and `shops` into `tileset.mbtiles`, at zooms 0 to 14. Without
`--source`, every source is rendered. Sources come from the same connection strings or
[configuration file](config-file/index.md) as for the tile server.

=== "PostgreSQL"

    Only table sources can be rendered, not function sources. All of them must come from one
    PostgreSQL connection.

Each source becomes one layer, named by its `layer_id` or its source id. A source with
[`layers`](sources-pg-layers.md) feeds several layers from one read instead, each with its own
features, zooms, attributes and draw order.

The same data and options give the same tiles, however many threads are used.

## Configuration file

The configuration file only describes the sources. The options of a run are command-line arguments,
and the file has no section for `martin generate`.

```bash
martin generate --config config.yaml --output-file tileset.pmtiles --max-zoom 14
```

=== "PostgreSQL"

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

    Of a [table source](sources-pg-tables.md), `martin generate` uses `schema`, `table`,
    `geometry_column`, `srid`, `id_column`, `properties`, `filter`, `minzoom`, `maxzoom`, `extent`,
    `buffer`, `layer_id` and [`layers`](sources-pg-layers.md). Settings that only affect serving, such as
    caching, are ignored.

## Options

| Option          | Default                                                           | Meaning                                                                                               |
|-----------------|-------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------|
| `--output-file` |                                                                   | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty                                  |
| `--source`      | all sources                                                       | A source id, repeatable                                                                               |
| `--format`      | `mlt`                                                             | `mlt` or `mvt`                                                                                        |
| `--encoding`    | `gzip` for MBtiles or MLT in PMTIles<br>`none` for MLT in PMTiles | MBTiles: `gzip`, `zlib` or `none`.<br>PMTiles: `gzip`, `br`, `zstd` or `none` for MVT, `none` for MLT |
| `--min-zoom`    | `0`                                                               | Lowest zoom, raised by a source's `minzoom`                                                           |
| `--max-zoom`    | `14`                                                              | Highest zoom, lowered by a source's `maxzoom`                                                         |
| `--bbox`        | whole world                                                       | Only features and tiles inside `min_lon,min_lat,max_lon,max_lat`                                      |
| `--memory`      | `2GiB`                                                            | Memory for sorting, e.g. `512MB` or `4GiB`, split over the threads                                    |
| `--temp-dir`    | system temp dir                                                   | Where sorted runs go, repeatable to spread them over several disks                                    |
| `--threads`     | CPU count                                                         | Worker threads                                                                                        |

The tile server's [configuration and PostgreSQL arguments](run-with-cli.md) also apply, such as
`--config` and `--pool-size`. A configuration file and connection strings cannot be combined.

`--bbox` filters features, which `martin cp` does not. A low-zoom tile at the edge of the box only holds
the features near the box, not everything the tile covers.

The highest zoom is 27. The tile grid can lower it further, since `extent × 2^zoom` must not exceed
`2^30`. With the default extent of 4096, that is zoom 18.

A failed or interrupted run removes the output file. Temporary files are always removed.

## Sizing a run

- `--memory` is split evenly over the render workers. Each gets at least 16 MiB and at most 4 GiB, so
  with many threads the total can exceed `--memory`. More memory means fewer, longer sorted runs. Up to
  256 runs are merged at once. With more, groups of runs are merged into longer ones first, which reads
  and writes the temporary data one more time.
- The temporary data holds a piece of every feature for every tile it touches, at every zoom. It grows
  with the zoom range and is usually several times the size of the input. Merging more than 256 runs
  needs room for one more copy.
- `--temp-dir` takes the sorted runs in turn. Directories on separate disks spread the I/O.
- `--threads` sets the number of render workers and encoders.

=== "PostgreSQL"

    Tables are read over at most one connection per thread, and no more than the pool's `pool_size`.

[Generation Architecture](architecture-generate.md) explains how a run reads, sorts, encodes and writes
the tiles, and what bounds its memory.
