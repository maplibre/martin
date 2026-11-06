---
description: Splitting a PostgreSQL table source into layers with their own features, zooms, attributes and draw order
icon: material/file-cog
tags:
  - postgresql
  - tile-sources
  - configuration
---

# Table Source Layers

--8<-- "generate-unstable.md"

`layers` is a key of a [PostgreSQL table source](sources-pg-tables.md). It turns one table into one or
more output layers, all fed by a single read of the table, each choosing its features, zooms, attributes
and draw order. The [bulk mode](martin-generate.md) of `martin cp` writes them into a tileset, and the
tile server renders them on request, see [Serving](#serving).

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
          id: false
```

Every feature is offered to each layer, which may take it or not. Inside each tile, the layers of one
table come in the order they are listed. Tables come in the order of `--source`, or ordered by source id
without it. Layer names must be unique across all sources, and a run holds at most 256 layers.

## Layer keys

| Key                               | Meaning                                                          | Details                                            |
|-----------------------------------|------------------------------------------------------------------|----------------------------------------------------|
| `minzoom`, `maxzoom`              | A number, or an expression for each feature's own zoom           | [Zooms](#zooms)                                    |
| `extent`, `buffer`, `clip_geom`   | Override the table source's                                      |                                                    |
| `simplify`, `simplify_at_maxzoom` | Pixels of tolerance when simplifying lines and polygons          | [Simplification](#simplification-and-minimum-size) |
| `min_size`, `min_size_at_maxzoom` | Pixels, below which lines and polygons are dropped               | [Simplification](#simplification-and-minimum-size) |
| `geometry`                        | `point`, `line` or `polygon`: only features of that type         | [Features](#features)                              |
| `where`                           | An expression: only features for which it is `true`              | [Features](#features)                              |
| `attributes`                      | Which properties to write, and under which names                 | [Attributes](#attributes)                          |
| `id`                              | `true` (default), `false`, or an expression                      | [Feature ids](#feature-ids)                        |
| `rules`                           | Different zooms, simplification and attributes for some features | [Rules](#rules)                                    |
| `sort_by`                         | The draw order of the layer's features in each tile              | [Draw order](#draw-order)                          |
| `label_grid`                      | The most points per grid square in one tile                      | [Per-tile operations](#per-tile-operations)        |
| `dedup_by`                        | Drops features repeating a key in one tile                       | [Per-tile operations](#per-tile-operations)        |
| `merge_multi`                     | One multi-geometry per group in one tile                         | [Per-tile operations](#per-tile-operations)        |
| `limit`                           | The most features of the layer in one tile                       | [Per-tile operations](#per-tile-operations)        |

`extent`, `buffer` and `clip_geom` default to the table source's, then to 4096, 64 and `true`.
`extent` must be a power of two from 64 to 2097152, and `buffer` less than half the `extent`.
With `clip_geom: false`, every tile a line or polygon reaches holds all of it, not just its part there.

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
Both are in pixels of a 256-pixel tile, whatever the `extent`, and default to the values
[Planetiler](https://github.com/onthegomap/planetiler) uses.

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

A [rule](#rules)'s `where` is not one of them.
A feature that matches no rule stays in the layer.

The database also applies the `geometry` and `where` of every layer at the zoom being read, so rows no
layer takes are never read. This needs a SQL equivalent for each `where` of the table. These have one:

- comparisons of a column with a literal,
- `==` or `!=` between two columns, unless both are floats,
- `in` with a list of literals, `has(feature.x)` and `'x' in feature`,
- `&&`, `||`, `!` and `? :`,
- `size`, `startsWith`, `endsWith`, `contains`, `lowerAscii` and `upperAscii` on a string column,
  compared with a literal.

Columns of type `bpchar` or `name` have none, as PostgreSQL compares them differently.
When a layer reads a `jsonb` property, no `where` has one, as any key may share a column's name.
Listing `attributes` without the `jsonb` property's keys avoids that.
Without a SQL equivalent, Martin reads every row and checks `where` itself, with the same result.

## Attributes

Without `attributes`, a layer writes all of the table source's properties. `[]` writes none.

A list copies those properties as they are. A map names each output attribute:

- a string is an expression, so `kind: class` writes the column `class` as `kind`,
- `{ value: ... }`, a number or a boolean is a literal,
- `{ expr: ..., minzoom: ..., maxzoom: ... }` is only written at those zooms.

In both, `name:*` copies the columns starting with `name:`, written `"name:*": "name:*"` in a map.
An expression that is `null` leaves the attribute out.

## Feature ids

`id: true` writes the `id_column` as the feature id, `id: false` writes none, and an expression
computes it. A computed id is only written when it is an integer of zero or more.

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

Without `sort_by`, the features of a layer keep the order the source reads them in.

=== "PostgreSQL"

    - A table is read in its physical order.
    - A view or foreign table with an integer `id_column` is read in 32 partitions by id remainder
      (`abs(id % 32)`), which interleaves the ids and overrides the view's `ORDER BY`.
    - Any other view is read in its own order.

    The tile server reads each tile's features in the order PostgreSQL finds them, which can
    differ from request to request, so only `sort_by` gives served tiles a fixed draw order.

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

--8<-- "images/pg/sort-by.svg"

Values order by type first: `null` (and failed evaluations), booleans, integers, floating-point
numbers, then strings. Any integer sorts before any floating-point number, so convert mixed numbers
with `double(...)` to compare them by value. `desc` reverses the whole order, so `null` comes last.

## Per-tile operations

Most layer keys act on one feature at a time. A per-tile operation acts on the layer's whole feature list
in one tile, after [`sort_by`](#draw-order) has ordered it and just before the tile is encoded. Each
operation is a layer key, named after the operation, with that operation's options. A table source or a
rule cannot set one.

```yaml
postgres:
  tables:
    shops:
      schema: public
      table: shops
      geometry_column: geom
      properties:
        name: text
        rank: int4
      layers:
        shops:
          sort_by: [rank]
          limit:
            - { maxzoom: 10, count: 50 }   # z0 to z10
            - { minzoom: 11, count: 200 }  # z11 and up
```

Operations run in this order, whatever order they are written in:

| Order | Operation     | Does                                                         |
|-------|---------------|--------------------------------------------------------------|
| 1     | `label_grid`  | Keeps the first `limit` points of each grid square           |
| 2     | `dedup_by`    | Drops features repeating an earlier feature's key            |
| 3     | `merge_multi` | Combines a group's features of one type into a multi-feature |
| 4     | `limit`       | Keeps the first `count` features, in order                   |

Every operation takes `minzoom` and `maxzoom`, numbers clamped to the layer's zooms. Outside them the
operation is skipped and features pass through unchanged. For different settings at different zooms,
give a list of option maps instead of one map; their zoom ranges must not overlap. An operation whose
zooms miss the layer's entirely is dropped with a warning.

Merging operations group features by the attributes in `group_by`, or by all the attributes they write
when it is unset. Values compare by type too, so `1` and `"1"` are different groups, and a feature
without a `group_by` attribute groups with others without it. A merged feature takes the place of the
first feature of its group, and that feature's other attributes. It keeps a feature id only if all of
its group has the same one.

Since operations keep the first features of the list, `sort_by` decides which features survive. A
layer without `sort_by` keeps the order features are read in, which the tile server does not fix, see
[Draw order](#draw-order).

### `label_grid`

Thins out points, such as labels, the way Planetiler's point label grid does. At each zoom it covers,
the world is cut into squares of `size` pixels, aligned to the world rather than to tiles. In each tile,
only the first `limit` points of each square are kept, in draw order, so give the layer a `sort_by`.

```yaml
postgres:
  tables:
    places:
      schema: public
      table: places
      geometry_column: geom
      buffer: 1024        # 64 px of a 4096 extent, the grid's size
      properties:
        name: text
        kind: text
        population: int8
      layers:
        places:
          geometry: point
          sort_by: [{ expr: "population", desc: true }]
          label_grid: { maxzoom: 14, size: 64, limit: 2 }
          rules:
            - where: "kind == 'capital'"
              label_grid: null   # capitals are never thinned out
            - where: "kind == 'village'"
              label_grid: { maxzoom: 14, size: 128, limit: 1 }
```

| Option               | Default             | Meaning                                         |
|----------------------|---------------------|-------------------------------------------------|
| `size`               | required, above 0   | Side of a square, in pixels of a 256-pixel tile |
| `limit`              | required, 1 or more | Points kept per square in each tile             |
| `minzoom`, `maxzoom` | the layer's zooms   | Where the grid applies; elsewhere, no limit     |

A rule may set its own `label_grid` for the features it takes, replacing the layer's, or `null` to turn it
off. Each point counts against its own `limit` in the square it falls in, so points of one layer with
different sizes share a count when their squares have the same column and row, as in Planetiler. A
multi-point is split into its points, each kept or dropped on its own. Lines and polygons pass through.
Squares smaller than 1/4096 of a tile do not limit anything.

A square crossing a tile edge is counted the same way in both tiles only when each sees all of its points,
so set the layer's `buffer` to at least `size` pixels. A warning tells you when it is smaller.

### `dedup_by`

Drops a feature when an earlier feature, in draw order, has the same key, such as a road name drawn many
times. The key is the list of `keys` values, CEL expressions evaluated per feature.

```yaml
dedup_by:
  keys: [name, ref]
  distance: 64   # only drop a duplicate within 64 px of a kept one
```

| Option               | Default           | Meaning                                                     |
|----------------------|-------------------|-------------------------------------------------------------|
| `keys`               | required          | CEL expressions forming the key                             |
| `distance`           | the whole tile    | Pixels within which a feature with the same key is dropped  |
| `minzoom`, `maxzoom` | the layer's zooms | Where it runs                                               |

Unlike `where`, `id` and attribute expressions, which read the source's properties, `keys` read the
feature's output attributes at this zoom by their output names, after `attributes`, `rules` and each
attribute's zooms, as Planetiler's layer post-processing sees features. A name the layer does not write
fails the run before rendering. A key whose values are all `null` is never a duplicate. A failed
evaluation counts as `null`, and is counted and logged once like other expressions.

With `distance`, each feature is placed at a representative point: a point itself, the middle of a
line's longest part, or a point inside a polygon.

### `merge_multi`

Combines the features of a group into one multi-point, multi-line or multi-polygon per geometry type,
without changing any geometry, the parts in draw order, as Planetiler's `mergeMultiPoint`,
`mergeMultiLineString` and `mergeMultiPolygon` do. A point alone in its group stays a point.

```yaml
merge_multi:
  geometry: [point]   # only combine points; lines and polygons pass through
  group_by: [kind]
```

| Option               | Default                      | Meaning                                    |
|----------------------|------------------------------|--------------------------------------------|
| `geometry`           | `[point, line, polygon]`     | The types to combine; others pass through  |
| `group_by`           | every attribute              | The attributes features must share         |
| `minzoom`, `maxzoom` | the layer's zooms            | Where it runs                              |

### `limit`

Keeps the first `count` features of the layer in the tile and drops the rest. `count: 0` leaves the
layer out of the tile. `limit: 100` is short for `limit: { count: 100 }`.

Unlike `max_feature_count` of a table source, which limits the rows read for a tile, `limit` limits the
features written to it.

## Expressions

Expressions are written in [CEL](https://cel.dev), the Common Expression Language. CEL looks like
the conditions of C, Java or JavaScript, but it has no statements, loops or side effects. Every
expression returns one value, and it always finishes.

A property is read by its name, such as `rank > 5`, or with `feature['name:en']` when the name is not
an identifier. A missing property is `null`. Strings take single or double quotes, so quote the whole
expression in YAML, for example `where: "kind == 'park'"`.

=== "Arithmetic"

    ```text
    7 / 2              // 3, integer division rounds toward zero
    -7 / 2             // -3
    7 % 3              // 1
    7.0 / 2.0          // 3.5
    2 * 1.5            // fails, integers and doubles do not mix in arithmetic
    double(2) * 1.5    // 3.0
    ```

=== "Comparison and logic"

    ```text
    3 >= 2 && 'a' != 'b'           // true
    !(1 > 2) || false              // true
    1 < 2.5                        // true, comparisons do mix integers and doubles
    'park' in ['park', 'forest']   // true
    ```

=== "Choice"

    ```text
    population > 1000000 ? 2 : 8      // 2 for a city of 5 million, else 8
    height > 0 ? height : 'unknown'   // the two branches can have different types
    1 ? 2 : 3                         // fails, the condition must be a boolean
    ```

    When an attribute gets different types in one tile, such as numbers and strings, all its values in
    that tile are written as strings.

=== "Conversion"

    ```text
    int(2.7)        // 2
    int('12')       // 12
    double('1.5')   // 1.5
    string(5)       // '5'
    ```

=== "Strings"

    ```text
    'Main' + ' St'                      // 'Main St'
    size('Straße')                      // 6
    'Main St'.startsWith('Main')        // true
    'Main St'.endsWith('St')            // true
    'Main St'.contains('n S')           // true
    'Main St'.lowerAscii()              // 'main st'
    ' Main St '.trim()                  // 'Main St'
    'Main St'.replace('St', 'Street')   // 'Main Street'
    'Main St'.substring(0, 4)           // 'Main'
    'Main St'.indexOf('St')             // 5
    ```

=== "Lists and maps"

    ```text
    'shop;cafe'.split(';')                          // ['shop', 'cafe']
    ['shop', 'cafe'].join(', ')                     // 'shop, cafe'
    'shop;cafe'.split(';').exists(k, k == 'shop')   // true
    [1, 2, 3].all(x, x > 0)                         // true
    [1, 2, 3].map(x, x * 2)                         // [2, 4, 6]
    [1, 2, 3].filter(x, x > 1)                      // [2, 3]
    [10, 20][1]                                     // 20
    {'a': 1}['a']                                   // 1
    ```

=== "null"

    ```text
    coalesce(feature['name:en'], name)   // the English name, else the name
    coalesce(null, null)                 // null
    ```

Martin adds these functions to CEL:

- `coalesce(a, b, ...)` returns its first argument that is not `null`.

Regular expressions (`matches`) and timestamps are not available.

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
          id: "osm_id * 10"
          attributes:
            kind: class
            label: { expr: "coalesce(feature['name:en'], name)", minzoom: 10 }
            source: { value: osm }
            rank: 3
```

## Serving

The tile server renders a table source with `layers` itself instead of asking PostGIS for an
`ST_AsMVT` tile. Each request reads the whole geometry of every feature that reaches the tile or its
buffer, and renders the tile bulk mode writes for the same `maxzoom`. [Serving tiles](architecture-generate.md#serving-tiles)
shows how.

- The highest zoom is the table source's `maxzoom`. Without one, it is the highest zoom the `extent`
  fits: 18 for an `extent` of 4096. A higher `maxzoom` fails at startup.
- Tiles are MVT, on the Web Mercator tile grid only, and the source takes no URL query parameters.
- `max_feature_count` limits the features read for each tile, or for each block with `prefetch`.
  The database must apply every `where` first, see [Features](#features), or startup fails.
  With it, `order_by` chooses which are kept. `order_by` alone changes nothing, as `sort_by` sets the
  draw order.
- `prefetch: N`, on the table source or as the default under `postgres`, renders the N×N block holding
  a tile on a cache miss, in one query, and caches all of its tiles. N is from 1 to 16, and 1 renders
  each tile alone. Blocks are aligned to multiples of N, so the tiles of one block share it. Without a
  tile cache, or outside its zooms, each tile is rendered alone. A table with `prefetch` needs `layers`.

```yaml
postgres:
  prefetch: 2
  tables:
    roads:
      schema: public
      table: roads
      geometry_column: geom
      id_column: osm_id
      prefetch: 4
      properties:
        class: text
      layers:
        roads: {}
```

## Validation

- **Reading the configuration** fails on invalid values, with their line and column: a zoom above 30,
  a negative `simplify`, a `minzoom` above `maxzoom`, an expression that is not valid CEL,
  `layer_id` together with `layers`, overlapping zoom ranges in one per-tile operation's list, or a
  per-tile operation on a table source or in a rule. Unknown keys are warned about.
- **Before rendering**, after connecting to the database, the run fails on unknown property names,
  duplicate layer names, more than 256 layers, a zoom above 27, and the settings that are not supported
  yet, and on a `group_by` attribute the layer does not write. It warns about a per-tile operation whose
  zooms miss the layer's, and drops it, about one that never acts on the layer's `geometry`, and about a
  `label_grid` with squares larger than the layer's `buffer`.

[`martin test`](martin-test.md) checks what the layers make of real rows of the database, and keeps it in
a spec file.
