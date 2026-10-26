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

A long-requested Martin feature is generating tiles on the fly from a feature source (raw data) inside
Martin itself, when serving tiles.
Until now, a source had to either already contain tiles (MBTiles, PMTiles) or generate them itself, like
PostgreSQL does with `ST_AsMVT`.

Martin adapted the tile generation method of [Planetiler](https://github.com/onthegomap/planetiler). Thank
you to Mike Barry ([@msbarry](https://github.com/msbarry)), the author of Planetiler, for an amazing tool,
and to its contributors. Planetiler is licensed under the
[Apache License 2.0](https://github.com/onthegomap/planetiler/blob/main/LICENSE). Martin also adapted a
similar [layer configuration](sources-pg-layers.md), which takes features from any source Martin supports
that can produce a stream of features. [Generation Architecture](architecture-generate.md) lists what
this takes from Planetiler.

As part of this effort, [`martin cp`](martin-cp.md) can now also build a whole tileset at once, like
Planetiler. By default it asks the source for each tile separately, so the source reads a feature again
for every tile it touches, at every zoom. With `--mode bulk`, it instead reads each source once, renders
every feature at every zoom inside Martin, and writes the tiles in order. For large tilesets this is much
faster, and it puts far less load on the source. That also saves money: Martin instances are stateless
and cheap to scale out, for example in Kubernetes, while scaling PostgreSQL is much harder and more
expensive.

The default `--mode auto` still picks `tile`. Once bulk mode covers enough, it will pick `bulk` whenever
it can.

It renders points, lines and polygons, single or multi, and skips empty geometries.
Any other geometry, such as a geometry collection, a triangle or a TIN, stops the run with an error.

## Usage

```bash
martin cp --mode bulk                   \
          --output-file tileset.mbtiles \
          --source roads,shops          \
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
and the file has no section for `martin cp`.

```bash
martin cp --mode bulk --config config.yaml --output-file tileset.pmtiles --max-zoom 14
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

    Of a [table source](sources-pg-tables.md), bulk mode uses `schema`, `table`,
    `geometry_column`, `srid`, `id_column`, `properties`, `filter`, `minzoom`, `maxzoom`, `extent`,
    `buffer`, `clip_geom`, `layer_id` and [`layers`](sources-pg-layers.md). Settings that only affect
    serving, such as caching, are ignored.

## Options

| Option                          | Default                                                           | Meaning                                                                                                 |
|---------------------------------|-------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------|
| `--output-file`                 |                                                                   | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty                                    |
| `--source`                      | all sources                                                       | Comma-separated source ids                                                                              |
| `--format`                      | `mlt`                                                             | `mlt` or `mvt`                                                                                          |
| `--encoding`                    | `gzip` for MBtiles or MVT in PMTIles<br>`none` for MLT in PMTiles | MBTiles: `gzip`, `zlib` or `none`.<br>PMTiles: `gzip`, `br`, `zstd` or `none` for MVT, `none` for MLT   |
| `--min-zoom`                    | `0`                                                               | Lowest zoom, raised by a source's `minzoom`                                                             |
| `--max-zoom`                    | `14`                                                              | Highest zoom, lowered by a source's `maxzoom`                                                           |
| `--zoom-levels`                 |                                                                   | Instead of `--min-zoom` and `--max-zoom`, zooms that do not skip a level, e.g. `5,6,7`                  |
| `--bbox`                        | whole world                                                       | Only tiles touching `min_lon,min_lat,max_lon,max_lat`, at most once                                     |
| `--memory`                      | `2GiB`                                                            | Memory for sorting, e.g. `512MB` or `4GiB`, split over the threads                                      |
| `--temp-dir`                    | system temp dir                                                   | Where sorted runs go, repeatable to spread them over several disks                                      |
| `--threads`                     | CPU count                                                         | Worker threads                                                                                          |
| `--mvt-quirk`                   |                                                                   | MVT: a [quirk](#mvt-quirks) to encode with, repeatable                                                  |
| `--mvt-first-vertex-like-wagyu` | off                                                               | MVT: a polygon ring in a tile that gets its whole polygon starts where `ST_AsMVTGeom`'s wagyu starts it |

The tile server's [configuration and PostgreSQL arguments](run-with-cli.md) also apply, such as
`--config` and `--pool-size`. A configuration file and connection strings cannot be combined.

Bulk mode refuses the `martin cp` options it does not read: `--url-query`, `--mbtiles-type`,
`--on-duplicate`, `--concurrency`, `--skip-agg-tiles-hash`, `--set-meta`, `--delete-meta`,
`--save-config` and `--max-feature-count`. Tile mode refuses `--memory`, `--temp-dir`, `--threads`,
`--mvt-quirk` and `--mvt-first-vertex-like-wagyu`.

`--bbox` picks tiles, as in tile mode, and each of them holds every feature it covers.
The scan reads everything the tiles of the lowest zoom cover, so with `--min-zoom 0` it reads the whole world.
Each zoom lower reads up to four times the area, and zooms below `--min-zoom` do not count.
For a small box, raise `--min-zoom` to keep the scan close to it.

The highest zoom is 27. The tile grid can lower it further, since `extent × 2^zoom` must not exceed
`2^30`. With the default extent of 4096, that is zoom 18.

A failed or interrupted run removes the output file. Temporary files are always removed.

### MVT quirks

Each `--mvt-quirk` changes how MVT tiles are encoded:

| Quirk              | Effect                                                                                                                                                                                     |
|--------------------|--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `keep-unused-keys` | Each layer lists every key the config gives it, used or not, in config order, as `ST_AsMVT` does. Other keys, such as those of a JSON column, follow in the order features first use them. |

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

    Tables are read over one connection per two threads, and no more than the pool's `pool_size`.

[Generation Architecture](architecture-generate.md) explains how a run reads, sorts, encodes and writes
the tiles, and what bounds its memory.
