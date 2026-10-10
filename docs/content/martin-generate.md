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
                --source shops                \
                --max-zoom 14                 \
                postgres://postgres@localhost:5432/db
```

Every table source becomes one layer, named by the source's `layer_id`, or by the source id.
Without `--source`, every table source is rendered.
Sources are configured exactly as for the tile server, with the same connection arguments and
[configuration file](config-file/index.md): `extent`, `buffer`, `minzoom`, `maxzoom`, `properties`,
`id_column`, `filter` and `clip_geom` of each table source apply. Function sources cannot be scanned.
Layer names must be unique across all sources.

## Several layers from one table

A table source with `layers` is scanned once and feeds each layer, named by its key and written in
this order. Layers and their settings are unstable and may change.

```yaml
postgres:
  tables:
    roads:
      schema: public
      table: roads
      srid: 4326
      geometry_column: geom
      id_column: osm_id
      properties:
        class: text
        name: text
        "name:de": text
      layers:
        roads:
          geometry: line
          attributes: [class]
        road_labels:
          minzoom: 10
          simplify: 2
          attributes: [name, "name:*"]
          id: drop
```

Each layer can set:

| Key                               | Meaning                                                                                         |
|-----------------------------------|-------------------------------------------------------------------------------------------------|
| `minzoom`, `maxzoom`              | A number, which narrows the table's zooms and `--min-zoom`/`--max-zoom`, or an expression       |
| `extent`, `buffer`, `clip_geom`   | Override the table's                                                                            |
| `simplify`, `simplify_at_maxzoom` | Pixels; default 0.1, and 1/16 at the layer's max zoom                                           |
| `min_size`, `min_size_at_maxzoom` | Pixels; lines and polygons smaller than this are dropped; default 1, and 1/16                   |
| `geometry`                        | `point`, `line` or `polygon`: only features of that type                                        |
| `where`                           | An expression: only features for which it is `true`                                             |
| `attributes`                      | A list of properties, `[]` for none; `name:*` takes the columns starting with `name:`; or a map |
| `id`                              | `keep` (default), `drop`, or `{ expr: ... }`                                                    |
| `rules`                           | Settings for the features matching a condition                                                  |

Only the properties some layer needs are read. A named attribute that is not a column is a key of
the table's `jsonb` column; a prefix such as `name:*` only matches columns, not `jsonb` keys.
A layer whose zooms are all outside the generated zooms is skipped with a warning.

### Expressions

Expressions are [CEL](https://cel.dev) over the feature's properties, such as `rank > 5` or
`feature['name:en']` for a name that is not an identifier.
An expression that fails on a feature, for example by dividing by zero, counts as `null` there;
the first failure of each expression is logged, and the run ends with how often each one failed.

A `minzoom` or `maxzoom` expression gives each feature its own zooms, within the layer's; `null`
keeps the layer's. An `id` expression is the feature id when it is a non-negative integer, and
leaves the feature without one otherwise.

```yaml
layers:
  places:
    where: "population > 1000"
    minzoom: "population > 1000000 ? 2 : 8"
    id: { expr: "osm_id * 10" }
    attributes:
      kind: class
      label: { expr: "name", minzoom: 10 }
      source: { value: osm }
      rank: 3
      "name:*": "name:*"
```

A map of `attributes` names each output attribute: a string is an expression (`class` copies the
column), `{ value: ... }`, a number or a boolean is a literal, and `{ expr: ..., minzoom: ...,
maxzoom: ... }` only appears at those zooms. An expression that is `null` leaves the attribute out.
`"name:*": "name:*"` copies the columns starting with `name:`.

### Rules

`rules` is a list; the first rule whose `where` holds for a feature applies to it, and a last rule
without `where` takes every other feature. A feature that matches no rule keeps the layer's settings.

```yaml
layers:
  roads:
    attributes:
      kind: class
    rules:
      - where: "class == 'motorway'"
        minzoom: 4
        attributes:
          kind: { value: major }
      - where: "class == 'path'"
        minzoom: 12
        simplify: 0
      - minzoom: 8
```

A rule can set `minzoom` and `maxzoom` (numbers or expressions), `simplify`, `min_size`, and
`attributes`. Its `simplify` and `min_size` also apply at the max zoom. Its attributes replace the
layer's attributes of the same name, at all zooms, and add the others.

### Not supported yet

These are rejected: `sort_by`, `geometry` other than `point`, `line` and `polygon`, and any `tile`
operation. `prefetch` only applies to the tile server.

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
| `--source`          | all tables      | A table source id, repeatable                                           |
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
