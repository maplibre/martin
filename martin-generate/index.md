# Generating Tilesets

> [!WARNING]
>
> **Not fully implemented**
>
> The bulk mode of `martin cp` is in the release binaries since 2.0.0-beta.6, but parts of what these pages describe are not implemented yet, and the configuration of `layers` may still change.
>
> - [x] Rendering points, lines and polygons, with fill ranges for covered tiles
> - [x] Tile grouping, MLT and MVT encoding, compression and deduplication
> - [x] Planning tables into layers and running a whole generation end to end
> - [x] The PostgreSQL source with partitioned whole-table scans
> - [x] Bulk mode in `martin cp`, and PMTiles output
> - [x] Layers with CEL expressions, `rules` and `sort_by`
> - [x] [OSM PBF files](<https://maplibre.org/martin/sources-osm/index.md>) as sources
> - [ ] Shapefile, GeoPackage and CSV files as sources
> - [x] `geometry` with Planetiler's `polygon_centroid`, `line_centroid` and `centroid`
> - [ ] `geometry` with Planetiler's `line_midpoint`, `polygon_centroid_if_convex`, `polygon_point_on_surface`, `point_on_line` and `innermost_point`
> - [x] [Per-tile operations](<https://maplibre.org/martin/sources-pg-layers/index.md#per-tile-operations>) (`merge_lines`, `merge_polygons`, `thinning`, `merge_multi`, `limit`)

A long-requested Martin feature is generating tiles on the fly from a feature source (raw data) inside Martin itself, when serving tiles. Until now, a source had to either already contain tiles (MBTiles, PMTiles) or generate them itself, like PostgreSQL does with `ST_AsMVT`.

Martin adapted the tile generation method of [Planetiler](<https://github.com/onthegomap/planetiler>). Thank you to Mike Barry ([@msbarry](<https://github.com/msbarry>)), the author of Planetiler, for an amazing tool, and to its contributors. Planetiler is licensed under the [Apache License 2.0](<https://github.com/onthegomap/planetiler/blob/main/LICENSE>). Martin also adapted a similar [layer configuration](<https://maplibre.org/martin/sources-pg-layers/index.md>), which takes features from any source Martin supports that can produce a stream of features. [Generation Architecture](<https://maplibre.org/martin/architecture-generate/index.md>) lists what this takes from Planetiler.

As part of this effort, [`martin cp`](<https://maplibre.org/martin/martin-cp/index.md>) can now also build a whole tileset at once, like Planetiler. By default it asks the source for each tile separately, so the source reads a feature again for every tile it touches, at every zoom. With `--mode bulk`, it instead reads each source once, renders every feature at every zoom inside Martin, and writes the tiles in order. For large tilesets this is much faster, and it puts far less load on the source. That also saves money: Martin instances are stateless and cheap to scale out, for example in Kubernetes, while scaling PostgreSQL is much harder and more expensive.

The default `--mode auto` still picks `tile`. Once bulk mode covers enough, it will pick `bulk` whenever it can.

It renders points, lines and polygons, single or multi, and skips empty geometries. Any other geometry, such as a geometry collection, a triangle or a TIN, stops the run with an error.

## Usage

```bash
martin cp --mode bulk                   \
          --output-file tileset.mbtiles \
          --source roads,shops          \
          --max-zoom 14                 \
          postgres://postgres@localhost:5432/db
```

This renders the sources `roads` and `shops` into `tileset.mbtiles`, at zooms 0 to 14. Without `--source`, every source is rendered. Sources come from the same connection strings or [configuration file](<https://maplibre.org/martin/config-file/index.md>) as for the tile server.

![martin cp --mode bulk prepares 2 tables, renders 7.5M features, then writes 1.3M tiles](<https://maplibre.org/martin/images/generate/bulk-progress.gif>)

Only table sources can be rendered, not function sources. All of them must come from one PostgreSQL connection.

Each source becomes one layer, named by its `layer_id` or its source id. A source with [`layers`](<https://maplibre.org/martin/sources-pg-layers/index.md>) feeds several layers from one read instead, each with its own features, zooms, attributes and draw order.

The same data and options give the same tiles, however many threads are used.

### Draw order

A source with `layers` orders the features of each layer by its [`sort_by`](<https://maplibre.org/martin/sources-pg-layers/index.md#draw-order>) and ignores `order_by`. A source without `layers` follows `order_by` (`--order-by` on the command line) of its PostgreSQL connection, as tile mode does, when the source has an `id_column`. Each tile then lists its features by id:

| `order_by` | Order of the ids `3`, `-2`, `NULL`, `1`, `-7` in each tile |
| --- | --- |
| `id-asc` | `-7`, `-2`, `1`, `3`, `NULL` |
| `id-desc` | `3`, `1`, `-2`, `-7`, `NULL` |

- Features without an id come last with either value.
- Features with equal ids keep the order the source reads them in.
- A tile drops a negative id, as `ST_AsMVT` does. The feature keeps its place, without an id.

Without `order_by`, or without an `id_column`, the features keep the [order the source reads them in](<https://maplibre.org/martin/sources-pg-layers/index.md#draw-order>).

## Configuration file

The configuration file only describes the sources. The options of a run are command-line arguments, and the file has no section for `martin cp`.

```bash
martin cp --mode bulk --config config.yaml --output-file tileset.pmtiles --max-zoom 14
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

Of a [table source](<https://maplibre.org/martin/sources-pg-tables/index.md>), bulk mode uses `schema`, `table`, `geometry_column`, `srid`, `id_column`, `properties`, `filter`, `minzoom`, `maxzoom`, `extent`, `buffer`, `clip_geom`, `layer_id`, [`layers`](<https://maplibre.org/martin/sources-pg-layers/index.md>) and the MLT encoder settings of [`convert_to_mlt`](<https://maplibre.org/martin/postprocessing/mlt/index.md#tuning-the-encoder>). Settings that only affect serving, such as caching, are ignored.

## Options

| Option | Default | Meaning |
| --- | --- | --- |
| `--output-file` |  | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty |
| `--source` | all sources | Comma-separated source ids |
| `--format` | `mlt` | `mlt` or `mvt`, and `mlt2` in a build with `unstable-mlt-v2` |
| `--encoding` | `gzip` for MBtiles or MVT in PMTIles<br>`none` for MLT in PMTiles | MBTiles: `gzip`, `zlib` or `none`.<br>PMTiles: `gzip`, `br`, `zstd` or `none` for MVT, `none` for MLT |
| `--min-zoom` | `0` | Lowest zoom, raised by a source's `minzoom` |
| `--max-zoom` | `14` | Highest zoom, lowered by a source's `maxzoom` |
| `--zoom-levels` |  | Instead of `--min-zoom` and `--max-zoom`, zooms that do not skip a level, e.g. `5,6,7` |
| `--bbox` | the sources' `bounds`, else whole world | Only tiles touching `min_lon,min_lat,max_lon,max_lat`, at most once |
| `--memory` | `2GiB` | Memory for sorting, e.g. `512MB` or `4GiB`, split over the threads |
| `--temp-dir` | system temp dir | Where sorted runs go, repeatable to spread them over several disks |
| `--threads` | CPU count | Worker threads |
| `--mvt-quirk` |  | MVT: a [quirk](<#mvt-quirks>) to encode with, repeatable |

The tile server's [configuration and PostgreSQL arguments](<https://maplibre.org/martin/run-with-cli/index.md>) also apply, such as `--config` and `--pool-size`. A configuration file and connection strings cannot be combined.

`--mbtiles-type`, `--skip-agg-tiles-hash`, `--set-meta` and `--delete-meta` work as in tile mode, for `.mbtiles` output only. Bulk mode refuses `--url-query`, `--on-duplicate`, `--concurrency` (its workers are `--threads`), `--save-config`, and `max_feature_count` in the config or as `--max-feature-count`. Tile mode refuses `--memory`, `--temp-dir`, `--threads` and `--mvt-quirk`.

`--format mlt2` writes the same layers as `--format mlt` in the v2 wire format, and reads the v2-only `convert_to_mlt` keys `allow_delta2` and `allow_rans_vertices`, which `--format mlt` refuses. Unlike tile mode, it does not keep nested `jsonb` values or M ordinates yet.

`--bbox` picks tiles, as in tile mode, and each of them holds every feature it covers. Without `--bbox`, bulk mode picks the tiles touching the union of the `bounds` of every selected source that renders at least one zoom of the run, clamped to latitudes ±85.0511°, as tile mode does:

- A source's `bounds` is the one its TileJSON shows: the configured `bounds`, else what the connection's `auto_bounds` computes. A `bounds` in the table comment replaces either.
- If one of these sources has no `bounds`, for example with `auto_bounds: skip` or when `auto_bounds: quick` stops after 5 seconds, bulk mode renders the whole world.
- The output records `--bbox`, or else this union, as its `bounds` metadata.

The scan reads everything the tiles of the lowest zoom cover, so with `--min-zoom 0` it reads the whole world. Each zoom lower reads up to four times the area, and zooms below `--min-zoom` do not count. For a small box, raise `--min-zoom` to keep the scan close to it.

The highest zoom is 27. The tile grid can lower it further, since `extent × 2^zoom` must not exceed `2^30`. With the default extent of 4096, that is zoom 18.

A failed or interrupted run removes the output file. Temporary files are always removed.

### Metadata

To set metadata such as `attribution`, `center` or `description`, add it to the [TileJSON in the table comment](<https://maplibre.org/martin/sources-pg-tables/index.md#tilejson-in-sql-comments>) of a rendered source. Bulk mode merges the comments of all rendered sources, in `--source` order or else by source id, and sets the computed keys itself:

| Key | Value in the output |
| --- | --- |
| `name` | The comments' names, without repeats, joined by `,`. Without one, the output's file stem, e.g. `tileset` for `tileset.mbtiles` |
| `attribution`, `description` | The comments' values, without repeats, one per line |
| `vector_layers` | One computed entry per layer with features. A comment entry with the same `id` replaces it, other comment entries are added |
| `bounds` | `--bbox`, or else the union of the sources' `bounds`, see [Options](<#options>) |
| `minzoom`, `maxzoom`, `format`, `generator` | Computed. `generator` is `martin cp v<version> (bulk)` |
| `compression` | MBTiles only: `gzip`, `deflate` for `--encoding zlib`, or none for `--encoding none` |
| `hash_algorithm`, `agg_tiles_hash`, `agg_tiles_hash_before_apply`, `agg_tiles_hash_after_apply` | Set by the MBTiles writer only, never by a comment. Pick the algorithm with `--set-meta hash_algorithm=xxh3` |
| `center`, `version`, `legend`, `template` and custom keys | The value of the first comment that sets it |

### MVT quirks

Each `--mvt-quirk` changes how MVT tiles are encoded, so it needs `--format mvt`:

| Quirk | Effect |
| --- | --- |
| `keep-unused-keys` | Each layer lists every key the config gives it, used or not, in config order, as `ST_AsMVT` does. Other keys, such as those of a JSON column, follow in the order features first use them. |
| `preserve-ring-start` | A polygon ring keeps its source start. By default, a ring in a tile that gets its whole polygon starts where `ST_AsMVTGeom`'s wagyu starts it. |

## Sizing a run

- `--memory` is split evenly over the render workers. Each gets at least 16 MiB and at most 4 GiB, so with many threads the total can exceed `--memory`. More memory means fewer, longer sorted runs. Up to 4096 runs are merged at once. With more, groups of runs are merged into longer ones first, which reads and writes the temporary data one more time.
- The temporary data holds a piece of every feature for every tile it touches, at every zoom. It grows with the zoom range and is usually several times the size of the input. Merging more than 4096 runs needs room for one more copy.
- `--temp-dir` takes the sorted runs in turn. Directories on separate disks spread the I/O.
- `--threads` sets the number of render workers and encoders.

Tables are read over one connection per two threads, and no more than the pool's `pool_size`.

[Generation Architecture](<https://maplibre.org/martin/architecture-generate/index.md>) explains how a run reads, sorts, encodes and writes the tiles, and what bounds its memory.
