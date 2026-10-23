---
description: Generating and checking specs of what table source layers make of real rows
icon: material/test-tube
tags:
  - tooling
  - postgres
  - testing
---

# Testing Layers

!!! warning "Not released"
    `martin test` is not in any release yet, and its spec file may still change.
    It is only built with the `unstable-generate` feature:

    ```bash
    cargo build --package martin --bin martin --features unstable-generate
    ```

`martin test` renders real rows through the [`layers`](sources-pg-layers.md) of table sources in
[bulk mode](martin-generate.md), and keeps the result in a spec file.

--8<-- "images/generate/test-overview.svg"

After a change to the layers, `run` shows what changed and `generate` updates the spec.

## Usage

```bash
martin test generate --config config.yaml --output roads.spec.yaml
martin test run --config config.yaml roads.spec.yaml
```

Tests PostgreSQL table sources with `layers` and an `id_column`, from one `postgres` section.

```yaml
postgres:
  connection_string: postgres://postgres@localhost:5432/db
  tables:
    roads:
      schema: public
      table: roads
      srid: 4326
      geometry_column: geom
      geometry_type: LINESTRING
      id_column: id
      filter: "class IN ('primary', 'residential')"
      properties:
        id: int8
        class: text
        name: text
      layers:
        roads:
          minzoom: "class == 'primary' ? 6 : 12"
          attributes: [class]
        road_labels:
          minzoom: 12
          where: "name != null"
          attributes: [name]
```

## Paths

Paths are the distinct, interesting cases this configuration can produce.

--8<-- "images/generate/test-rows.svg"

```text
   Searching 12 paths through the layers of 1 sources, max zoom 14
       FOUND [   0.014s] roads  roads: every feature  row 1
       FOUND [   0.018s] roads  roads: `class == 'primary' ? 6 : 12`: `class == 'primary'` true  row 1
       FOUND [   0.070s] roads  roads: `class == 'primary' ? 6 : 12`: `class == 'primary'` false  row 2
       FOUND [   0.072s] roads  roads: `class` is 'residential'  row 2
      NO ROW [   0.071s] roads  roads: kept only at z14 by `min_size_at_maxzoom`
      NO ROW [   0.004s] roads  roads: smaller than `min_size` at every zoom
       FOUND [   0.012s] roads  roads: crosses a z14 tile edge  row 1
       FOUND [   0.071s] roads  road_labels: where `name != null`  row 1
       FOUND [   0.075s] roads  road_labels: not where `name != null`  row 3
      NO ROW [   0.078s] roads  road_labels: kept only at z14 by `min_size_at_maxzoom`
      NO ROW [   0.003s] roads  road_labels: smaller than `min_size` at every zoom
       FOUND [   0.010s] roads  road_labels: crosses a z14 tile edge  row 1
   Rendering 3 cases in bulk mode and served
         NEW [   0.223s] roads 1  roads: every feature
         NEW [   0.126s] roads 2  roads: `class == 'primary' ? 6 : 12`: `class == 'primary'` false
         NEW [   0.119s] roads 3  road_labels: not where `name != null`
------------
     Summary [   1.079s] 12 paths: 8 found, 4 no row, 3 cases: 3 new
```

A row on several paths is one case. `NO ROW`: no row takes the path.

## Spec file

```yaml title="roads.spec.yaml"
max_zoom: 14
cases:
- source: roads
  paths:
  - "road_labels: not where `name != null`"
  - "roads: every feature"
  - "roads: `class == 'primary' ? 6 : 12`: `class == 'primary'` false"
  - "roads: `class` is 'residential'"
  input: # (1)!
    class: residential
    geometry: LINESTRING(16.34 48.22,16.341 48.221)
    id: 3
    name: null
  output: # (2)!
  - layer: roads
    id: 3
    geometry: line
    zooms: 12-14
    attributes:
      class: residential
```

1. The row's `id_column`, the columns the layers read, and its geometry as WKT. `run` reports `ROW CHANGED` when
   the row no longer matches.
2. What bulk mode writes: one entry per feature and layer, with its zooms and attributes.

## Generating

Without a spec, or with `--accept`, `generate` writes every case. Otherwise it opens a review in a
terminal, or prints the diffs and exits with `1`.

## Running

After changing `minzoom` to `"class == 'primary' ? 7 : 12"`:

```text
    Starting 3 cases of roads.spec.yaml with config.yaml, max zoom 14
        FAIL [   0.236s] roads 1  roads: every feature

--- OUTPUT: roads 1
    -spec +bulk mode now
     - layer: roads
       id: 1
       geometry: line
    -  zooms: 6-14
    +  zooms: 7-14
       attributes:
         class: primary
     - layer: road_labels

        PASS [   0.171s] roads 2  roads: `class == 'primary' ? 6 : 12`: `class == 'primary'` false
        PASS [   0.173s] roads 3  road_labels: not where `name != null`
------------
     Summary [   0.688s] 3 cases run: 2 passed, 1 failed
        FAIL [   0.236s] roads 1  roads: every feature
error: 1 of 3 cases did not pass
  to see the new output and accept it: martin test generate -c config.yaml -o roads.spec.yaml
```

`run` exits with `0` when all cases pass, `1` otherwise, and `2` on an error.
