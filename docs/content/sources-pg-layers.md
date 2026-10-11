---
description: Splitting a PostgreSQL table source into layers with their own features, zooms, attributes and draw order
icon: material/file-cog
tags:
  - postgresql
  - tile-sources
  - configuration
---

# Table Source Layers

!!! warning "Not released, not fully implemented"
    `martin generate` is not in any release yet, and parts of what these pages describe are not
    implemented yet. It is only built with the `unstable-generate` feature:

    ```bash
    cargo build --package martin --bin martin --features unstable-generate
    ```

    Not supported yet:

    - [ ] `geometry` other than `point`, `line` and `polygon`, rejected naming the layer
    - [ ] `tile` operations (`merge_lines`, `merge_polygons`, `merge_multi`, `label_grid`, `dedup_by`, `limit`), rejected naming the layer
    - [ ] `prefetch`, which is ignored

`layers` is a key of a [PostgreSQL table source](sources-pg-tables.md). It turns one table into one or
more output layers, all fed by a single read of the table, each choosing its features, zooms, attributes
and draw order. Only [`martin generate`](martin-generate.md) uses it so far, and the tile server ignores it.

A table source without `layers` is one layer, named by its `layer_id` or by its source id, holding all
of its `properties` and its feature ids. A table source cannot set both `layer_id` and `layers`.

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

Every feature is offered to each layer, which may take it or not. Inside each tile, the layers of one
table come in the order they are listed. Tables come in the order of `--source`, or ordered by source id
without it. Layer names must be unique across all sources, and a run holds at most 256 layers.

## Layer keys

| Key                               | Meaning                                                          | Details                                            |
|-----------------------------------|------------------------------------------------------------------|----------------------------------------------------|
| `minzoom`, `maxzoom`              | A number, or an expression for each feature's own zoom           | [Zooms](#zooms)                                    |
| `extent`, `buffer`                | Override the table source's                                      |                                                    |
| `simplify`, `simplify_at_maxzoom` | Pixels of tolerance when simplifying lines and polygons          | [Simplification](#simplification-and-minimum-size) |
| `min_size`, `min_size_at_maxzoom` | Pixels, below which lines and polygons are dropped               | [Simplification](#simplification-and-minimum-size) |
| `geometry`                        | `point`, `line` or `polygon`: only features of that type         | [Features](#features)                              |
| `where`                           | An expression: only features for which it is `true`              | [Features](#features)                              |
| `attributes`                      | Which properties to write, and under which names                 | [Attributes](#attributes)                          |
| `id`                              | `keep` (default), `drop`, or `{ expr: ... }`                     | [Feature ids](#feature-ids)                        |
| `rules`                           | Different zooms, simplification and attributes for some features | [Rules](#rules)                                    |
| `sort_by`                         | The draw order of the layer's features in each tile              | [Draw order](#draw-order)                          |

`extent` and `buffer` default to the table source's, then to 4096 and 64.
`buffer` must be less than half the `extent`.

## Properties

A layer can only use the table source's `properties`, both as attributes and in expressions. An
auto-published table has all of its columns as properties, but a table source in the configuration file
only has the properties it lists. The `id_column` is not a property. To use it in an expression, list it
under `properties` as well.

When the table has a `jsonb` property, a name that is not a column is read as a key of it. Otherwise,
an unknown name fails the run. A prefix such as `name:*` only matches columns, not `jsonb` keys.

Only the columns that some layer uses are read.

## Zooms

Zooms narrow from the run down to each feature: every level can raise the lowest zoom and lower the
highest, never widen what the level above allows.

--8<-- "images/generate/zooms.svg"

1. `--min-zoom` and `--max-zoom` set the run's zooms. A table source's `minzoom` and `maxzoom` narrow
   them. A table source left with no zoom fails the run.
2. A layer's `minzoom` and `maxzoom` numbers narrow the table source's. A layer left with no zoom is
   skipped with a warning.
3. A `minzoom` or `maxzoom` expression gives each feature its own zooms within the layer's. Its result
   is clamped to the layer's zooms, and `null` keeps the layer's bound. A feature whose `minzoom` ends
   up above its `maxzoom` is left out of the layer.
4. An attribute with `minzoom` or `maxzoom` is only written at those zooms of the feature.

A [rule](#rules) can set `minzoom` and `maxzoom` too. For the features it takes, they replace the
layer's expressions.

The highest zoom is 27.

## Simplification and minimum size

Lines and polygons are simplified at each zoom, and dropped once they are smaller than `min_size`.
Both are in pixels of a 256-pixel tile, whatever the `extent`.

| Key        | Below the layer's max zoom | At the layer's max zoom                           |
|------------|----------------------------|---------------------------------------------------|
| `simplify` | `simplify`, default 0.1    | `simplify_at_maxzoom`, default `simplify` or 1/16 |
| `min_size` | `min_size`, default 1      | `min_size_at_maxzoom`, default `min_size` or 1/16 |

The layer's max zoom is its highest zoom after narrowing, not the highest zoom of a single feature.
A rule's `simplify` and `min_size` apply at every zoom, including the max zoom.

## Features

A feature belongs to a layer when all of these hold:

1. the table source's `filter`, which the database applies while reading,
2. the layer's `geometry` type, if set,
3. the layer's `where` expression, if set. `null` and failed evaluations count as `false`.

A rule's `where` does not filter features: it only picks the rule.

## Attributes

Without `attributes`, a layer writes all of the table source's properties. `[]` writes none.

A list copies those properties as they are, and `name:*` copies the columns starting with `name:`.
A map names each output attribute:

- a string is an expression, so `kind: class` writes the column `class` as `kind`,
- `{ value: ... }`, a number or a boolean is a literal,
- `{ expr: ..., minzoom: ..., maxzoom: ... }` is only written at those zooms,
- `"name:*": "name:*"` copies the columns starting with `name:`.

An expression that is `null` leaves the attribute out.

## Feature ids

`id: keep` writes the `id_column` as the feature id, `id: drop` writes none, and `id: { expr: ... }`
computes it. A computed id that is not a non-negative integer leaves the feature without an id.

## Rules

`rules` is a list. The first rule whose `where` holds for a feature applies to it, and a last rule
without `where` takes every other feature. A feature that matches no rule keeps the layer's settings.

```yaml
postgres:
  tables:
    roads:
      schema: public
      table: roads
      geometry_column: geom
      properties:
        class: text
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

A rule can set:

- `minzoom` and `maxzoom`, numbers or expressions. They replace the layer's zoom expressions and are
  clamped to the layer's zooms. A rule's expression that is `null` falls back to the layer's.
- `simplify` and `min_size`, at every zoom.
- `attributes`, as a map without `*` prefixes. They replace the layer's attributes of the same name,
  at all zooms, and add the others.

## Draw order

Without `sort_by`, the features of a layer keep the order they are read in:

- a table in its physical order,
- a view or foreign table with an integer `id_column` by that id, which overrides the view's `ORDER BY`,
- any other view in its own order.

`sort_by` is a list of expressions, each a string or `{ expr: ..., desc: true }`, compared in turn.
Features that compare equal keep the order they are read in.

```yaml
postgres:
  tables:
    buildings:
      schema: public
      table: buildings
      geometry_column: geom
      properties:
        z_order: int4
        height: float8
      layers:
        buildings:
          sort_by: ["z_order", { expr: "height", desc: true }]
```

Values order by type first: `null` (and failed evaluations), booleans, integers, floating-point
numbers, then strings. Any integer sorts before any floating-point number, so convert mixed numbers
with `double(...)` to compare them by value. `desc` reverses the whole order, so `null` comes last.

## Expressions

Expressions are [CEL](https://cel.dev) over the feature's properties, such as `rank > 5`, or
`feature['name:en']` for a name that is not an identifier. A missing property is `null`.
Besides the CEL standard library and its string functions, `coalesce(a, b, ...)` returns its first
argument that is not `null`.

| Expression           | Result                                                              |
|----------------------|---------------------------------------------------------------------|
| `where`              | A boolean, where `null` counts as `false`                           |
| `minzoom`, `maxzoom` | An integer, clamped to the layer's zooms, or `null` for the layer's |
| `id`                 | A non-negative integer, else the feature has no id                  |
| attribute            | Any value, where `null` leaves the attribute out                    |
| `sort_by`            | Any value, see [draw order](#draw-order)                            |

An expression that fails on a feature, for example by dividing by zero or by returning a string where
a zoom is needed, counts as `null` there. The first failure of each expression is logged, and the run
ends with how often each one failed.

```yaml
postgres:
  tables:
    places:
      schema: public
      table: places
      geometry_column: geom
      properties:
        osm_id: int8
        class: text
        name: text
        "name:en": text
        population: int8
      layers:
        places:
          where: "population > 1000"
          minzoom: "population > 1000000 ? 2 : 8"
          id: { expr: "osm_id * 10" }
          attributes:
            kind: class
            label: { expr: "coalesce(feature['name:en'], name)", minzoom: 10 }
            source: { value: osm }
            rank: 3
```

## Validation

- **Reading the configuration** fails on invalid values, with their line and column: a zoom above 30,
  a negative `simplify`, a `minzoom` above `maxzoom`, an expression that is not valid CEL, or
  `layer_id` together with `layers`. Unknown keys are warned about.
- **Before rendering**, after connecting to the database, the run fails on unknown property names,
  duplicate layer names, more than 256 layers, a zoom above 27, and the settings that are not supported
  yet.
