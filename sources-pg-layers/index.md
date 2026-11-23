# Table Source Layers

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
> - [x] [OSM PBF files](<https://maplibre.org/martin/sources-osm/index.md>) and [shapefiles](<https://maplibre.org/martin/sources-shapefile/index.md>) as sources
> - [ ] GeoPackage and CSV files as sources
> - [x] `geometry` with Planetiler's `polygon_centroid`, `line_centroid`, `centroid`, `polygon_point_on_surface` and `polygon_centroid_if_convex`
> - [ ] `geometry` with Planetiler's `line_midpoint`, `point_on_line` and `innermost_point`
> - [x] [Per-tile operations](<https://maplibre.org/martin/sources-pg-layers/index.md#per-tile-operations>) (`merge_lines`, `merge_polygons`, `thinning`, `merge_multi`, `limit`)

`layers` is a key of a [PostgreSQL table source](<https://maplibre.org/martin/sources-pg-tables/index.md>). It turns one table into one or more output layers, all fed by a single read of the table, each choosing its features, zooms, attributes and draw order. The [bulk mode](<https://maplibre.org/martin/martin-generate/index.md>) of `martin cp` writes them into a tileset, and the tile server renders them on request, see [Serving](<#serving>).

A table source without `layers` is one layer, named by its `layer_id` or by its source id, holding all of its `properties` and its feature ids. A table source cannot set both `layer_id` and `layers`.

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

Every feature is offered to each layer, which may take it or not. Inside each tile, the layers of one table come in the order they are listed. Tables come in the order of `--source`, or ordered by source id without it. Layers with the same name, or the same [`layer`](<#combining-layers>), are written into one output layer, which comes where its first layer would. A run holds at most 256 layers.

## Layer keys

| Key | Meaning | Details |
| --- | --- | --- |
| `layer` | The output layer it writes into; the layer's key when unset | [Combining layers](<#combining-layers>) |
| `minzoom`, `maxzoom` | A number, or an expression for each feature's own zoom | [Zooms](<#zooms>) |
| `extent`, `buffer`, `clip_geom` | Override the table source's |  |
| `simplify`, `simplify_at_maxzoom` | Pixels of tolerance when simplifying lines and polygons | [Simplification](<#simplification-and-minimum-size>) |
| `min_size`, `min_size_at_maxzoom` | Pixels, below which lines and polygons are dropped | [Simplification](<#simplification-and-minimum-size>) |
| `geometry` | Only features of some types, or a point computed from each | [Geometry](<#geometry>) |
| `where` | An expression: only features for which it is `true` | [Features](<#features>) |
| `attributes` | Which properties to write, and under which names | [Attributes](<#attributes>) |
| `id` | `true` (default), `false`, or an expression | [Feature ids](<#feature-ids>) |
| `rules` | Different zooms, simplification and attributes for some features | [Rules](<#rules>) |
| `sort_by` | The draw order of the layer's features in each tile | [Draw order](<#draw-order>) |
| `merge_lines` | Joins lines whose ends meet in one tile | [Per-tile operations](<#per-tile-operations>) |
| `merge_polygons` | Unions polygons of a group in one tile | [Per-tile operations](<#per-tile-operations>) |
| `thinning` | Drops features crowded out by earlier ones in one tile | [Per-tile operations](<#per-tile-operations>) |
| `merge_multi` | One multi-geometry per group in one tile | [Per-tile operations](<#per-tile-operations>) |
| `limit` | The most features of the layer in one tile | [Per-tile operations](<#per-tile-operations>) |

`extent`, `buffer` and `clip_geom` default to the table source's, then to 4096, 64 and `true`. `extent` must be a power of two from 64 to 2097152, and `buffer` less than half the `extent`. With `clip_geom: false`, every tile a line or polygon reaches holds all of it, not just its part there.

## Properties

A layer can only use the table source's `properties`, both as attributes and in expressions. An auto-published table has all of its columns as properties, but a table source in the configuration file only has the properties it lists. The `id_column` is not a property. To use it in an expression, list it under `properties` as well.

When the table has a `jsonb` property, a name that is not a column is read as a key of it. Otherwise, an unknown name fails the run. A prefix such as `name:*` only matches columns, not `jsonb` keys.

Only the columns that some layer uses are read.

## Zooms

Zooms narrow from the run down to each feature: every level can raise the lowest zoom and lower the highest, never widen what the level above allows.

1. `--min-zoom` and `--max-zoom` set the run's zooms. A table source's `minzoom` and `maxzoom` narrow them. A table source left with no zoom fails the run.
2. A layer's `minzoom` and `maxzoom` numbers narrow the table source's. A layer left with no zoom is skipped with a warning.
3. A `minzoom` or `maxzoom` expression gives each feature its own zooms within the layer's. Its result is clamped to the layer's zooms, and `null` keeps the layer's bound. A feature whose `minzoom` ends up above its `maxzoom` is left out of the layer.
4. An attribute with `minzoom` or `maxzoom` is only written at those zooms of the feature.

A [rule](<#rules>) can set `minzoom` and `maxzoom` too. For the features it takes, they replace the layer's expressions.

The highest zoom is 27.

## Simplification and minimum size

Lines and polygons are simplified at each zoom, and dropped once they are smaller than `min_size`. Both are in pixels of a 256-pixel tile, whatever the `extent`, and default to the values [Planetiler](<https://github.com/onthegomap/planetiler>) uses.

| Key | Below the layer's max zoom | At the layer's max zoom |
| --- | --- | --- |
| `simplify` | `simplify`, default 0.1 | `simplify_at_maxzoom`, default `simplify` or 1/16 |
| `min_size` | `min_size`, default 1 | `min_size_at_maxzoom`, default `min_size` or 1/16 |

The layer's max zoom is its highest zoom after narrowing, not the highest zoom of a single feature. A rule's `simplify` and `min_size` apply at every zoom, including the max zoom.

## Features

A feature belongs to a layer when all of these hold:

1. the table source's `filter`, which the database applies while reading,
2. the types the layer's [`geometry`](<#geometry>) takes, if set,
3. the layer's `where` expression, if set. `null` and failed evaluations count as `false`.

A [rule](<#rules>)'s `where` is not one of them. A feature that matches no rule stays in the layer.

The database also applies the geometry types and `where` of every layer at the zoom being read, so rows no layer takes are never read. This needs a SQL equivalent for each `where` of the table. These have one:

- comparisons of a column with a literal,
- `==` or `!=` between two columns, unless both are floats,
- `in` with a list of literals, `has(feature.x)` and `'x' in feature`,
- `&&`, `||`, `!` and `? :`,
- `size`, `startsWith`, `endsWith`, `contains`, `lowerAscii` and `upperAscii` on a string column, compared with a literal.

Columns of type `bpchar` or `name` have none, as PostgreSQL compares them differently. When a layer reads a `jsonb` property, no `where` has one, as any key may share a column's name. Listing `attributes` without the `jsonb` property's keys avoids that. Without a SQL equivalent, Martin reads every row and checks `where` itself, with the same result.

### Geometry

`geometry` sets the features a layer takes and what it draws for each, with Planetiler's names:

| `geometry` | Takes | Draws |
| --- | --- | --- |
| `point` | points | the points |
| `line` | lines | the lines |
| `polygon` | polygons | the polygons |
| `polygon_centroid` | polygons | the centroid, a point |
| `line_centroid` | lines | the centroid, a point |
| `centroid` | all | the centroid, a point |
| `polygon_point_on_surface` | polygons | a point on its surface |
| `polygon_centroid_if_convex` | polygons | the centroid if convex, else a point on its surface |

The centroid is area-weighted for polygons, length-weighted for lines, and the mean for points, over all parts of a multi-geometry. It can lie outside a concave polygon. A point on the surface lies inside the polygon: the middle of the widest stretch inside it of a horizontal line through the middle of its bounding box, and for a multi-polygon, that of the part where this stretch is widest. As in Planetiler, a polygon is convex when it has no holes and no turn of its outline goes the other way by more than a thousandth of its largest turn; a multi-polygon never is. Each point is computed once per feature, in Web Mercator, and drawn like any point: only in the tiles it reaches, with no `min_size`, and [per-tile operations](<#per-tile-operations>) see a point.

`geometry` can also be a list, as Planetiler lists several `features` with one filter and the same attributes. Each feature is drawn by the first entry that takes its type, and only once; the entries share the layer's `where`, zooms and attributes. All entries of a list draw points, and an entry whose types the entries before it all take is refused.

```yaml
layers:
  pois:
    geometry: [point, polygon_point_on_surface]
    where: "amenity == 'cafe'"
```

This layer draws a cafe point as itself and a cafe polygon as a point on its surface, and leaves out cafe lines.

## Attributes

Without `attributes`, a layer writes all of the table source's properties. `[]` writes none.

A list copies those properties as they are. A map names each output attribute:

- a string is an expression, so `kind: class` writes the column `class` as `kind`,
- `{ value: ... }`, a number or a boolean is a literal,
- `{ expr: ..., minzoom: ..., maxzoom: ... }` is only written at those zooms.
- `{ expr: ..., hidden: true }` is not written to tiles, see [Hidden attributes](<#hidden-attributes>).

In both, `name:*` copies the columns starting with `name:`, written `"name:*": "name:*"` in a map. An expression that is `null` leaves the attribute out.

### Hidden attributes

[Per-tile operations](<#per-tile-operations>) read a feature's attributes, not the source's columns. When an operation needs a value that the tiles do not, add it as an attribute with `hidden: true`. The operation reads it, and Martin leaves it out of the tile. Planetiler's temporary tags do the same job.

Use a hidden attribute to:

- key a `thinning` by a value you do not publish,
- keep features apart in `merge_lines`, or in `merge_polygons` without `group_by`. Both merge only features whose attributes are all equal, hidden attributes included.

This layer keeps one point per street and house number in each tile, and writes only the house number:

```yaml
postgres:
  tables:
    addresses:
      schema: public
      table: addresses
      geometry_column: geom
      properties:
        street: text
        housenumber: text
      layers:
        housenumber:
          geometry: point
          attributes:
            housenumber: housenumber
            _street: { expr: street, hidden: true }
          thinning: { method: tile, keys: [_street, housenumber] }
```

`merge_lines` can reverse a joined line, which breaks one-way roads. This layer gives each one-way road its own `_unmerged` value, so it never merges. Two-way roads have no `_unmerged`, since a `null` expression leaves the attribute out, and merge as before:

```yaml
postgres:
  tables:
    roads:
      schema: public
      table: roads
      geometry_column: geom
      properties:
        osm_id: int8
        highway: text
        oneway: text
      layers:
        transportation:
          geometry: line
          attributes:
            class: highway
            _unmerged: { expr: "oneway == 'yes' ? osm_id : null", hidden: true }
          merge_lines: {}
```

A hidden attribute is:

- left out of every tile and of the `vector_layers` metadata,
- left out of the MVT key table, also with `--mvt-quirk keep-unused-keys`,
- hidden for the whole output layer. When a layer or one of its rules hides an attribute, Martin also hides it in the features of other layers that [write into the same output layer](<#combining-layers>).

## Feature ids

`id: true` writes the `id_column` as the feature id, `id: false` writes none, and an expression computes it. A computed id is only written when it is an integer of zero or more.

## Rules

`rules` is a list. The first rule whose `where` holds for a feature applies to it, and a last rule without `where` takes every other feature. A feature that matches no rule keeps the layer's settings.

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

- `minzoom` and `maxzoom`, numbers or expressions. They replace the layer's zoom expressions and are clamped to the layer's zooms. A rule's expression that is `null` falls back to the layer's.
- `simplify` and `min_size`, at every zoom.
- `attributes`, as a map without `*` prefixes. They replace the layer's attributes of the same name, at all zooms, and add the others.

## Draw order

Without `sort_by`, the features of a layer keep the order the source reads them in. An output layer that [combines layers](<#combining-layers>) holds the features of its first layer, then those of the next.

- A table is read in its physical order.
- A view or foreign table with an integer `id_column` is read in 32 partitions by id remainder (`abs(id % 32)`), which interleaves the ids and overrides the view's `ORDER BY`.
- Any other view is read in its own order.

The tile server reads each tile's features in the order PostgreSQL finds them, which can differ from request to request, so only `sort_by` gives served tiles a fixed draw order.

`sort_by` is a list of expressions, each a string or `{ expr: ..., desc: true }`, compared in turn. Features that compare equal keep the order they are read in.

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

Values order by type first: `null` (and failed evaluations), booleans, integers, floating-point numbers, then strings. Any integer sorts before any floating-point number, so convert mixed numbers with `double(...)` to compare them by value. `desc` reverses the whole order, so `null` comes last.

## Per-tile operations

Most layer keys act on one feature at a time. A per-tile operation acts on the layer's whole feature list in one tile, after [`sort_by`](<#draw-order>) has ordered it and just before the tile is encoded. Each operation is a layer key, named after the operation, with that operation's options. A table source cannot set one, and a rule only a grid [`thinning`](<#thinning>).

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

| Order | Operation | Does |
| --- | --- | --- |
| 1 | `merge_lines` | Joins lines whose ends meet and attributes are equal |
| 2 | `merge_polygons` | Unions a group's polygons |
| 3 | `thinning` | Drops features an earlier one with the same key crowds out |
| 4 | `merge_multi` | Combines a group's features of one type into a multi-feature |
| 5 | `limit` | Keeps the first `count` features, in order |

Every operation takes `minzoom` and `maxzoom`, numbers clamped to the layer's zooms. Outside them the operation is skipped and features pass through unchanged. For different settings at different zooms, give a list of option maps instead of one map; their zoom ranges must not overlap. An operation whose zooms miss the layer's entirely is dropped with a warning.

Merging operations group features by the values of attributes: `merge_lines` by all of them, `merge_polygons` by those in `group_by`, or all of them when it is unset, and `merge_multi` by those in `group_by`. Values compare by type too, so `1` and `"1"` are different groups, and a feature without a `group_by` attribute groups with others without it. A merged feature takes the place of the first feature of its group. It keeps a feature id only if all of its group has the same one.

Since operations keep the first features of the list, `sort_by` decides which features survive. A layer without `sort_by` keeps the order features are read in, which the tile server does not fix, see [Draw order](<#draw-order>).

After a bulk run, Martin logs each operation of each layer with the tiles it ran on and the features it took and kept, and warns about groups a merge could not make valid:

```text
INFO Ran `limit` layer.id=roads tiles=22 features.input=264 features.output=202
```

The output does not depend on `--threads`. Tiles served with or without `prefetch` equal the bulk ones of `--format mvt` for layers with a `sort_by`.

### `merge_lines`

Joins lines whose attributes are all equal into longer lines, so a road cut into many segments becomes one, which lets clients place labels along it. Where exactly two line ends meet, at the same tile coordinates, the lines are joined; where three or more meet, as at a junction, they are not. The joined lines are simplified by `tolerance`, the same way `simplify` does it, clipped to the tile and `buffer`, and dropped when their length inside is below `min_length`. The lines of equal attributes become one line or multi-line feature, or nothing if no line is left. Joining may reverse a line's direction. To join lines that differ in some attribute, leave it out of `attributes` at those zooms. To keep lines apart by a value you do not publish, use a [hidden attribute](<#hidden-attributes>).

```yaml
merge_lines:
  - { maxzoom: 10, min_length: 1, tolerance: 0.5 }
  - { minzoom: 11, maxzoom: 13, min_length: 0.25 }
```

| Option | Default | Meaning |
| --- | --- | --- |
| `min_length` | 0 | Pixels; shorter merged lines are dropped |
| `tolerance` | 0 | Pixels; simplifies the merged lines, 0 keeps every vertex |
| `buffer` | the layer's buffer | Pixels around the tile kept after merging, at most the layer's |
| `minzoom`, `maxzoom` | the layer's zooms | Where it runs |

Planetiler's `mergeLineStrings` joins lines the same way at plain joints, and also joins strokes through junctions and drops short stubs and loops, which `merge_lines` does not.

Each tile joins, simplifies and measures only what it holds of the lines, in the tile and `buffer`. A line ending just past a tile edge can be shorter than `min_length` in the next tile and is dropped there, and the two tiles' simplified lines can cross their shared edge at different points.

### `merge_polygons`

Unions the polygons of a group, such as adjacent land use areas of one kind, as Planetiler's `mergeNearbyPolygons` does, or `mergeOverlappingPolygons` with `distance: 0`. The polygons are first clipped to the tile and `buffer`. With a `distance`, they are grown by half of it, merged and shrunk again, which joins polygons closer than `distance`. Holes smaller than `min_hole_area` are then filled, polygons smaller than `min_area` dropped, and the result snapped to the tile grid. Each group becomes one polygon or multi-polygon feature, or nothing. A group whose union is not valid once snapped keeps its features as they were, and the first such group is logged. The run summary counts them.

```yaml
merge_polygons:
  maxzoom: 12
  min_area: 4        # px²
  min_hole_area: 1   # px²
  distance: 0.5      # px
  group_by: [kind]
```

| Option | Default | Meaning |
| --- | --- | --- |
| `min_area` | 0 | Square pixels; smaller merged polygons are dropped |
| `min_hole_area` | 0 | Square pixels; smaller holes are filled |
| `distance` | 0 | Pixels; closer polygons are joined, 0 joins touching ones |
| `buffer` | the layer's buffer | Pixels around the tile kept after merging, at most the layer's |
| `group_by` | every attribute | The attributes polygons must share |
| `minzoom`, `maxzoom` | the layer's zooms | Where it runs |

A covered tile's fill is a square over the tile and its buffer, merged like any other polygon.

Each tile merges only what it holds of the polygons, in the tile and `buffer`, so `min_area` counts only that part. A polygon reaching just past a tile edge can be dropped from the next tile.

### `thinning`

Drops a feature when an earlier kept feature, in draw order, crowds it out, so give the layer a `sort_by`. `method` says what crowds a feature out:

| `method` | A feature is dropped when |
| --- | --- |
| `grid` | its square already holds `limit` kept points |
| `distance` | a kept feature lies within `distance` of it |
| `tile` | a kept feature is anywhere in the tile |

With `keys`, a list of CEL expressions, only features with the same key values crowd each other out, such as a road name drawn many times. Without `keys`, every feature competes with every other.

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
          thinning:
            - { maxzoom: 9, method: grid, size: 64, limit: 2 }
            - { minzoom: 10, method: distance, distance: 64, keys: [name] }
          rules:
            - where: "kind == 'capital'"
              thinning: null   # capitals are never thinned out by the grid
            - where: "kind == 'village'"
              thinning: { maxzoom: 9, method: grid, size: 128, limit: 1 }
```

| Option | Default | Meaning |
| --- | --- | --- |
| `method` | required | `grid`, `distance` or `tile` |
| `size` | required by `grid` | Side of a square above 0, in pixels of a 256-pixel tile |
| `limit` | every point; `grid` needs it or `rank` | Points kept per square and key in each tile, 1 or more |
| `rank` | none | `grid`: attribute each kept point's position is written to |
| `rank_offset` | `0` | `grid`: added to the position `rank` writes |
| `distance` | required by `distance` | Pixels within which a feature with the same key is dropped |
| `keys` | one key for all; required by `tile` | CEL expressions forming the key |
| `minzoom`, `maxzoom` | the layer's zooms | Where it runs |

`grid` thins out points, such as labels, the way Planetiler's point label grid does. At each zoom it covers, the world is cut into squares of `size` pixels, aligned to the world rather than to tiles. Each point counts against its own `limit` in the square it falls in, so points of one layer with different sizes share a count when their squares have the same column and row, as in Planetiler. A multi-point is split into its points, each kept or dropped on its own. Lines and polygons pass through. Squares smaller than one unit of the layer's `extent` do not limit anything. A square crossing a tile edge is counted the same way in both tiles only when each sees all of its points, so set the layer's `buffer` to at least `size` pixels. A warning tells you when it is smaller.

With `rank`, each point the grid keeps gets its position among the kept points of its square and key in the tile, in draw order and counting from 1, plus `rank_offset`, as Planetiler's OpenMapTiles `place` layer writes `rank` from its `gridrank`. A point that already has the attribute keeps its value, but still takes a position. Without `limit`, the grid keeps every point and only ranks them.

```yaml
place:
  geometry: point
  sort_by: [{ expr: "rank" }]
  thinning: { maxzoom: 12, method: grid, size: 128, rank: rank, rank_offset: 10 }
  rules:
    - where: "rank == null"   # unranked towns and villages are thinned as well as ranked
      thinning:
        - { maxzoom: 8, method: grid, size: 128, limit: 4 }
        - { minzoom: 9, maxzoom: 12, method: grid, size: 128, limit: 8 }
```

A rule may set its own grid `thinning` for the features it takes, with a `limit` and without `keys` or `rank`, replacing the layer's grid, or `null` to turn the grid off for them. The layer's `rank` still ranks the points a rule's grid keeps. `distance` and `tile` thinning see every feature of the layer; to spare some features, give them a key of only `null`s.

`distance` places each feature at a representative point of what the tile holds of it, in the tile and its buffer: a point itself, the middle of a line's longest part, or a point inside a polygon. A line crossing a tile edge has a different one in each tile. Features on either side of a tile edge crowd each other out the same way in both tiles only when the layer's `buffer` is at least `distance` pixels.

`tile` keeps the first feature of each key in the tile and its buffer. That can be a feature that only reaches into the buffer, which leaves no feature of its key inside the tile.

Unlike `where`, `id` and attribute expressions, which read the source's properties, `keys` read the feature's output attributes at this zoom by their output names, after `attributes`, `rules` and each attribute's zooms, as Planetiler's layer post-processing sees features. A name the layer does not write fails the run before rendering. A feature whose key values are all `null` is never thinned out. A failed evaluation counts as `null`, and is counted and logged once like other expressions.

### `merge_multi`

Combines the features of a group into one multi-point, multi-line or multi-polygon per geometry type, without changing any geometry, the parts in draw order, as Planetiler's `mergeMultiPoint`, `mergeMultiLineString` and `mergeMultiPolygon` do. A point alone in its group stays a point. Every feature of a type in `geometry` keeps only its `group_by` attributes, even one alone in its group.

Each tile combines only the features it holds, in the tile and its buffer, so a multi-feature never spans tiles and a point in the buffer is a part in both tiles.

```yaml
merge_multi:
  geometry: [point]   # only combine points; lines and polygons pass through
  group_by: [kind]
```

| Option | Default | Meaning |
| --- | --- | --- |
| `geometry` | `[point, line, polygon]` | The types to combine; others pass through |
| `group_by` | required, 1 or more | The attributes features share and keep |
| `minzoom`, `maxzoom` | the layer's zooms | Where it runs |

### `limit`

Keeps the first `count` features of the layer in the tile and drops the rest. `count: 0` leaves the layer out of the tile. `limit: 100` is short for `limit: { count: 100 }`.

Features in the tile's buffer count too, so a feature near a tile edge can be kept in its own tile and dropped from the buffer of the next.

Unlike `max_feature_count` of a table source, which limits the rows read for a tile, `limit` limits the features written to it.

## Combining layers

`layer` names the output layer a layer writes into. Without it, the layer's key is its output layer. Layers of one output layer, from one source or from several sources of one run, are merged into that one layer in each tile. The key stays the layer's name in errors, warnings and [`martin test`](<https://maplibre.org/martin/martin-test/index.md>) paths.

```yaml
postgres:
  tables:
    parks:
      schema: public
      table: parks
      geometry_column: geom
      id_column: osm_id
      properties:
        class: text
        name: text
      layers:
        park:
          geometry: polygon
          buffer: 4
          attributes: [class]
        park_label:
          layer: park
          where: "name != null"
          geometry: polygon_point_on_surface
          buffer: 256
          attributes: [name]
```

Each layer keeps its own `where`, `geometry`, zooms, `buffer`, `clip_geom`, simplification, `attributes`, `id` and `rules`, and its features keep their ids. With the `parks` source above, each tile holds one `park` layer, with each park's polygon and, for a named park, the point labeling it.

The output layer has some settings once, so its layers must agree on them, or the run fails naming two of them:

| Key | Must be |
| --- | --- |
| `extent` | Equal on all of them, after defaulting to the source's or 4096 |
| `sort_by`, `merge_lines`, `merge_polygons`, `thinning`, `merge_multi`, `limit` | Equal wherever set; set on one layer, it applies to all |

`sort_by` and the per-tile operations act on the features of all the layers together, within the zooms of all of them. An operation without its own `buffer` keeps the widest `buffer` of the layers. The `vector_layers` metadata lists the output layer once, with the attributes of all its layers, and the zooms its tiles have it at.

## Expressions

Expressions are written in [CEL](<https://cel.dev>), the Common Expression Language. CEL looks like the conditions of C, Java or JavaScript, but it has no statements, loops or side effects. Every expression returns one value, and it always finishes.

A property is read by its name, such as `rank > 5`, or with `feature['name:en']` when the name is not an identifier. A missing property is `null`. Strings take single or double quotes, so quote the whole expression in YAML, for example `where: "kind == 'park'"`.

```text
7 / 2              // 3, integer division rounds toward zero
-7 / 2             // -3
7 % 3              // 1
7.0 / 2.0          // 3.5
2 * 1.5            // fails, integers and doubles do not mix in arithmetic
double(2) * 1.5    // 3.0
```

```text
3 >= 2 && 'a' != 'b'           // true
!(1 > 2) || false              // true
1 < 2.5                        // true, comparisons do mix integers and doubles
'park' in ['park', 'forest']   // true
```

```text
population > 1000000 ? 2 : 8      // 2 for a city of 5 million, else 8
height > 0 ? height : 'unknown'   // the two branches can have different types
1 ? 2 : 3                         // fails, the condition must be a boolean
```

When an attribute gets different types in one tile, such as numbers and strings, all its values in that tile are written as strings.

```text
int(2.7)        // 2
int('12')       // 12
double('1.5')   // 1.5
string(5)       // '5'
```

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

```text
'12a'.matches('^[0-9]+$')                              // false
matches('12', '^[0-9]+$')                              // true
regex.replace('5a;7b', '[^0-9;]', '')                  // '5;7'
regex.replace('B 39', '([A-Z]+) ([0-9]+)', '\\2 \\1')  // '39 B'
```

Patterns use the syntax of Rust's [`regex`](<https://docs.rs/regex/latest/regex/#syntax>) crate, which has no lookaround and no backreferences.

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

```text
coalesce(feature['name:en'], name)   // the English name, else the name
coalesce(null, null)                 // null
```

Martin adds these functions to CEL:

- `coalesce(a, b, ...)` returns its first argument that is not `null`.
- `max(a, b, ...)`, `max(list)` or `list.max()` returns the largest value, `null` for an empty list, as `max(ref.split(';').map(r, size(r)))` for the longest part of a `ref`.
- `min(a, b, ...)`, `min(list)` or `list.min()` returns the smallest value, `null` for an empty list.
- `regex.replace(text, pattern, replacement)` replaces each match of `pattern` in `text`, as cel-go's `regex.replace`: `\1` to `\9` in `replacement` insert a group, `\0` the whole match and `\\` a backslash (written `'\\1'` in a CEL string). It returns `null` for a `null` text.
- `text.isLatin()` is `true` when `text` holds only Latin letters and characters that are no letters, such as digits, punctuation and spaces, as Planetiler's `containsOnlyLatinCharacters`. It is `false` for `null` and `''`.
- `text.removeLatin()` drops the Latin letters from `text`, then brackets left empty and the spaces, dots, slashes and dashes at both ends, as Planetiler's `removeLatinCharacters`. It returns `null` for `null` and when dropping Latin letters leaves only blanks, so `'日本 (Japan)'.removeLatin()` is `'日本'`. A `text` without Latin letters is only trimmed, so `''.removeLatin()` is `''`.
- The `area(unit)` function calculates the total surface area of a geometry, returning 0.0 for points and lines. It measures the shape on a spherical model of the Earth using its average radius and ignores any internal holes. For context, measuring a one-degree square at the equator will return approximately 12,364 square kilometers.

You must specify a valid unit using either abbreviations or common spellings, such as `km2` or `square meters`. Supported units include metric (`m2`, `km2`, `ha`, `a`) and imperial or nautical units (`ft2`, `yd2`, `mi2`, `ac`, `nm2`). You can also use Web Mercator squares, where `z0 ti2` represents the entire world as `1`, and `z0 px2` represents the entire world as `65,536` (though Planetiler handles this by dividing instead).

Limitations are that you cannot use it within a `where` clause (because no direct SQL equivalent), and you cannot use it within `thinning` keys.

Timestamps are not available. A `where` calling `matches`, `regex.replace`, `max`, `min`, `isLatin` or `removeLatin` has no SQL equivalent.

OpenMapTiles' `name:latin` is the first name written only in Latin letters, and `name:nonlatin` the rest of a name that is not:

```yaml
attributes:
  name: name
  "name:latin": >-
    name.isLatin() ? name
    : feature['name:en'].isLatin() ? feature['name:en']
    : int_name.isLatin() ? int_name
    : feature['name:de'].isLatin() ? feature['name:de']
    : null
  "name:nonlatin": "name.isLatin() ? null : name.removeLatin()"
```

| Expression | Result |
| --- | --- |
| `where` | A boolean, where `null` counts as `false` |
| `minzoom`, `maxzoom` | An integer, clamped to the layer's zooms, or `null` for the layer's |
| `id` | A non-negative integer, else the feature has no id |
| attribute | Any value, where `null` leaves the attribute out |
| `sort_by` | Any value, see [draw order](<#draw-order>) |

An expression that fails on a feature, for example by dividing by zero or by returning a string where a zoom is needed, counts as `null` there. The first failure of each expression is logged, and the run ends with how often each one failed.

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

The tile server renders a table source with `layers` itself instead of asking PostGIS for an `ST_AsMVT` tile. Each request reads the whole geometry of every feature that reaches the tile or its buffer, and renders the tile bulk mode writes for the same `maxzoom`. [Serving tiles](<https://maplibre.org/martin/architecture-generate/index.md#serving-tiles>) shows how.

- The highest zoom is the table source's `maxzoom`. Without one, it is the highest zoom the `extent` fits: 18 for an `extent` of 4096. A higher `maxzoom` fails at startup.
- Tiles are MVT, on the Web Mercator tile grid only, and the source takes no URL query parameters.
- `max_feature_count` limits the features read for each tile, or for each block with `prefetch`. The database must apply every `where` first, see [Features](<#features>), or startup fails. With it, `order_by` chooses which are kept. `order_by` alone changes nothing, as `sort_by` sets the draw order.
- `prefetch: N`, on the table source or as the default under `postgres`, renders the N×N block holding a tile on a cache miss, in one query, and caches all of its tiles. N is from 1 to 16, and 1 renders each tile alone. Blocks are aligned to multiples of N, so the tiles of one block share it. Without a tile cache, or outside its zooms, each tile is rendered alone. A table with `prefetch` needs `layers`.

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

- **Reading the configuration** fails on invalid values, with their line and column: a zoom above 30, a negative `simplify`, a `minzoom` above `maxzoom`, an expression that is not valid CEL, `layer_id` together with `layers`, overlapping zoom ranges in one per-tile operation's list, a `thinning` option its `method` does not take, or a per-tile operation on a table source or, except a grid `thinning`, in a rule. Unknown keys are warned about.
- **Before rendering**, after connecting to the database, the run fails on unknown property names, layers of one output layer that disagree on `extent`, `sort_by` or a per-tile operation, more than 256 layers, a zoom above 27, and the settings that are not supported yet, and on a `group_by` attribute the layer does not write. It warns about a per-tile operation whose zooms miss the layer's, and drops it, about one that never acts on the layer's `geometry`, and about a grid `thinning` with squares larger than the layer's `buffer`.

[`martin test`](<https://maplibre.org/martin/martin-test/index.md>) checks what the layers make of real rows of the database, and keeps it in a spec file.
