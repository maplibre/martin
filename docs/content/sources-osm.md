---
description: Generating tilesets from OpenStreetMap PBF files with martin cp --mode bulk
icon: material/map-legend
tags:
  - openstreetmap
  - tile-sources
  - configuration
---

# OpenStreetMap PBF Sources

!!! note
    Only the [bulk mode](martin-generate.md) of `martin cp` and [`martin test`](martin-test.md) can process these sources.
    The tile serving mode can only serve them after they are brought into a more suitable format.

Martin reads one `.osm.pbf` file per source, using [layers](sources-pg-layers.md).

```yaml
osm:
  sources:
    berlin:
      path: berlin-latest.osm.pbf
      tags: [highway, name, building]
      layers:
        roads:
          geometry: line
          where: "highway in ['motorway', 'trunk', 'primary']"
          attributes: [highway, name]
        buildings:
          geometry: polygon
          minzoom: 14
          where: "building != null"
          attributes: []
```

```bash
martin cp --mode bulk --config config.yaml --source berlin --output-file berlin.mbtiles
```

A run renders `osm` and [`shapefile`](sources-shapefile.md) sources together, or database sources, not both.
[Building a basemap from OpenStreetMap](recipe-osm-basemap.md) has one possible basemap schema.

## Settings

| Key                                   | Meaning                                                              |
|---------------------------------------|----------------------------------------------------------------------|
| `path`                                | The `.osm.pbf` file                                                  |
| `tags`                                | The tag keys the layers read, each a string property                 |
| `layers`                              | Output layers, as for [table sources](sources-pg-layers.md)          |
| `minzoom`, `maxzoom`                  | Narrow the zooms of `--min-zoom` and `--max-zoom`                    |
| `extent`, `buffer`, `clip_geom`       | Defaults for the contained layers                                    |

## Supported Features and handling

| OSM object                                           | Feature                      | `osm_type` | Feature id  |
|------------------------------------------------------|------------------------------|------------|-------------|
| Node with tags                                       | point                        | `node`     | id * 10 + 1 |
| Way with tags                                        | line, polygon or both, below | `way`      | id * 10 + 2 |
| Relation with `type=multipolygon` or `type=boundary` | polygon or multipolygon      | `relation` | id * 10 + 3 |

Martin leaves out objects without tags, and other relations.
Martin reads a way as Planetiler does, whatever its other tags:

| Way                                     | Polygon | Line |
|-----------------------------------------|---------|------|
| Not closed                              | no      | yes  |
| Closed, no `area` tag or another value  | yes     | yes  |
| Closed, `area=yes`                      | yes     | no   |
| Closed, `area=no`                       | no      | yes  |
| Closed, fewer than 4 nodes              | no      | yes  |
| Closed, fewer than 4 nodes, `area=yes`  | no      | no   |

Layers of polygons take the polygon and layers of lines the line, both with the way's feature id.
A closed `highway=pedestrian` way can be both a street and a street area.
Layers of any geometry, such as `geometry: centroid`, take the polygon only, as does a layer whose `geometry`
list takes polygons and lines, so it draws the way once.
For a closed way, `line_centroid` is the centroid of the polygon it encloses, as in Planetiler.

A relation's member ways are joined into rings.
A ring inside an odd number of others is a hole, whatever its role.
A way or relation whose nodes are not in the file is left out, as is a relation with a member way that is not.

Each key in `tags` is a string property, `null` where the object does not have the tag or its value is empty.
`osm_type` is always one.
An expression reading a key not in `tags` fails the run. A tag such as `name:en` is read with
`feature['name:en']`.

## Reading only what the layers take

The `where` and `geometry` of every layer are checked while the file is read, as PostgreSQL checks them for table
sources:

1. An object type no layer can take is not decoded. Without layers that take lines or polygons, the node
   locations are not even loaded.
2. A block of the file whose strings cannot hold a match, such as one without the string `highway` for
   `highway == 'primary'`, is skipped before it is decompressed in full.
3. Each object's tags are checked before the locations of a way's nodes are looked up.
4. A closed way is read as a line only if a layer with `geometry: line` may take it.

This needs the same equivalent of `where` as [table sources](sources-pg-layers.md#features) do. Without one, Martin
reads every tagged object and checks `where` itself, with the same result.

## Node locations

Martin builds ways and relations from node locations, which it loads before it reads the file. For a PBF of up to
512 MiB, it keeps them in memory, about 30 bytes per node. For a larger file, it keeps them in a file of about
5 bytes per node in the first `--temp-dir`, and deletes that file when the run ends.

## Draw order

Without `sort_by`, features keep the order of the file, nodes before ways, then relations by id.
The output does not depend on `--threads`.
