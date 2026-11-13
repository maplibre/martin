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
    Only the [bulk mode](martin-generate.md) of `martin cp` reads these sources; the tile server does not serve them.

An `osm` source is one `.osm.pbf` file and the [layers](sources-pg-layers.md) made from it. Martin reads the file
with [osmflow](https://github.com/nyurik/osmflow).

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

A run renders either `osm` sources or database sources, not both.
[Building a basemap from OpenStreetMap](recipe-osm-basemap.md) has a complete basemap schema.

## Source keys

| Key                                   | Meaning                                                              |
|---------------------------------------|----------------------------------------------------------------------|
| `path`                                | The `.osm.pbf` file                                                  |
| `tags`                                | The tag keys the layers read, each a string property                 |
| `layers`                              | Output layers, as for [table sources](sources-pg-layers.md)          |
| `minzoom`, `maxzoom`                  | Narrow the zooms of `--min-zoom` and `--max-zoom`                    |
| `extent`, `buffer`, `clip_geom`       | Defaults of the layers, as for table sources                         |

## Features

| OSM object                                           | Feature                                                 | `osm_type` | Feature id  |
|------------------------------------------------------|---------------------------------------------------------|------------|-------------|
| Node with tags                                       | point                                                   | `node`     | id × 10 + 1 |
| Way with tags                                        | line, or polygon if closed and its tags make it an area | `way`      | id × 10 + 2 |
| Relation with `type=multipolygon` or `type=boundary` | polygon or multipolygon                                 | `relation` | id × 10 + 3 |

Objects without tags, and other relations, are not features. Whether a closed way is an area follows osmflow's
`AreaRules`: `building`, `landuse`, `natural` other than `coastline`, `area=yes` and so on.
A relation's member ways are joined into rings; a ring inside an odd number of others is a hole, whatever its role.
A way or relation whose nodes are not in the file is left out, as is a relation with a member way that is not.

Each key in `tags` is a string property, `null` where the object does not have the tag. `osm_type` is always one.
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

This needs the same equivalent of `where` as [table sources](sources-pg-layers.md#features) do. Without one, Martin
reads every tagged object and checks `where` itself, with the same result.

## Node locations

Ways and relations are built from node locations, loaded before the file is read. Up to 512 MiB of PBF, they are
kept in memory, about 30 bytes per node. Larger files keep them in a file of about 5 bytes per node in the first
`--temp-dir`, deleted when the run ends.

## Draw order

Without `sort_by`, features keep the order of the file, nodes before ways, then relations by id.
The output does not depend on `--threads`.
