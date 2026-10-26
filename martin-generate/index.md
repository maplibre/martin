# Generating Tilesets

> [!WARNING]
>
> **Not released or fully implemented**
>
> The bulk mode of `martin cp` is not in any release yet, and parts of what these pages describe are not implemented yet. It is only built with the `unstable-generate` feature:
>
> ```bash
> cargo build --package martin --bin martin --features unstable-generate
> ```
>
> Not implemented yet:
>
> - [ ] Rendering points, lines and polygons, with fill ranges for covered tiles
> - [ ] Tile grouping, MLT and MVT encoding, compression and deduplication
> - [ ] Planning tables into layers and running a whole generation end to end
> - [ ] The PostgreSQL source with partitioned whole-table scans
> - [ ] Bulk mode in `martin cp`, and PMTiles output
> - [ ] Layers with CEL expressions, `rules` and `sort_by`
> - [ ] OSM PBF, shapefile, GeoPackage and CSV files as sources
> - [ ] `geometry` other than `point`, `line` and `polygon`
> - [ ] `tile` operations (`merge_lines`, `merge_polygons`, `merge_multi`, `label_grid`, `dedup_by`, `limit`)

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

Only table sources can be rendered, not function sources. All of them must come from one PostgreSQL connection.

Each source becomes one layer, named by its `layer_id` or its source id. A source with [`layers`](<https://maplibre.org/martin/sources-pg-layers/index.md>) feeds several layers from one read instead, each with its own features, zooms, attributes and draw order.

The same data and options give the same tiles, however many threads are used.

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

Of a [table source](<https://maplibre.org/martin/sources-pg-tables/index.md>), bulk mode uses `schema`, `table`, `geometry_column`, `srid`, `id_column`, `properties`, `filter`, `minzoom`, `maxzoom`, `extent`, `buffer`, `clip_geom`, `layer_id` and [`layers`](<https://maplibre.org/martin/sources-pg-layers/index.md>). Settings that only affect serving, such as caching, are ignored.

## Options

| Option | Default | Meaning |
| --- | --- | --- |
| `--output-file` |  | A new `.pmtiles` archive, or an `.mbtiles` file that is new or empty |
| `--source` | all sources | Comma-separated source ids |
| `--format` | `mlt` | `mlt` or `mvt` |
| `--encoding` | `gzip` for MBtiles or MVT in PMTIles<br>`none` for MLT in PMTiles | MBTiles: `gzip`, `zlib` or `none`.<br>PMTiles: `gzip`, `br`, `zstd` or `none` for MVT, `none` for MLT |
| `--min-zoom` | `0` | Lowest zoom, raised by a source's `minzoom` |
| `--max-zoom` | `14` | Highest zoom, lowered by a source's `maxzoom` |
| `--zoom-levels` |  | Instead of `--min-zoom` and `--max-zoom`, zooms that do not skip a level, e.g. `5,6,7` |
| `--bbox` | whole world | Only tiles touching `min_lon,min_lat,max_lon,max_lat`, at most once |
| `--memory` | `2GiB` | Memory for sorting, e.g. `512MB` or `4GiB`, split over the threads |
| `--temp-dir` | system temp dir | Where sorted runs go, repeatable to spread them over several disks |
| `--threads` | CPU count | Worker threads |
| `--mvt-quirk` |  | MVT: a [quirk](<#mvt-quirks>) to encode with, repeatable |

The tile server's [configuration and PostgreSQL arguments](<https://maplibre.org/martin/run-with-cli/index.md>) also apply, such as `--config` and `--pool-size`. A configuration file and connection strings cannot be combined.

Bulk mode refuses the `martin cp` options it does not read: `--url-query`, `--mbtiles-type`, `--on-duplicate`, `--concurrency`, `--skip-agg-tiles-hash`, `--set-meta`, `--delete-meta`, `--save-config` and `--max-feature-count`. Tile mode refuses `--memory`, `--temp-dir`, `--threads` and `--mvt-quirk`.

`--bbox` picks tiles, as in tile mode, and each of them holds every feature it covers. The scan reads everything the tiles of the lowest zoom cover, so with `--min-zoom 0` it reads the whole world. Each zoom lower reads up to four times the area, and zooms below `--min-zoom` do not count. For a small box, raise `--min-zoom` to keep the scan close to it.

The highest zoom is 27. The tile grid can lower it further, since `extent × 2^zoom` must not exceed `2^30`. With the default extent of 4096, that is zoom 18.

A failed or interrupted run removes the output file. Temporary files are always removed.

### MVT quirks

Each `--mvt-quirk` changes how MVT tiles are encoded, so it needs `--format mvt`:

| Quirk | Effect |
| --- | --- |
| `keep-unused-keys` | Each layer lists every key the config gives it, used or not, in config order, as `ST_AsMVT` does. Other keys, such as those of a JSON column, follow in the order features first use them. |
| `preserve-ring-start` | A polygon ring keeps its source start. By default, a ring in a tile that gets its whole polygon starts where `ST_AsMVTGeom`'s wagyu starts it. |

## Sizing a run

- `--memory` is split evenly over the render workers. Each gets at least 16 MiB and at most 4 GiB, so with many threads the total can exceed `--memory`. More memory means fewer, longer sorted runs. Up to 256 runs are merged at once. With more, groups of runs are merged into longer ones first, which reads and writes the temporary data one more time.
- The temporary data holds a piece of every feature for every tile it touches, at every zoom. It grows with the zoom range and is usually several times the size of the input. Merging more than 256 runs needs room for one more copy.
- `--temp-dir` takes the sorted runs in turn. Directories on separate disks spread the I/O.
- `--threads` sets the number of render workers and encoders.

Tables are read over one connection per two threads, and no more than the pool's `pool_size`.

[Generation Architecture](<https://maplibre.org/martin/architecture-generate/index.md>) explains how a run reads, sorts, encodes and writes the tiles, and what bounds its memory.
