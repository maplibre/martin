---
description: Generating tilesets from ESRI shapefiles with martin cp --mode bulk
icon: material/vector-polygon
tags:
  - tile-sources
  - configuration
---

# Shapefile Sources

!!! note
    Only the [bulk mode](martin-generate.md) of `martin cp` and [`martin test`](martin-test.md) can process these sources.
    The tile serving mode can only serve them after they are brought into a more suitable format.

A `shapefile` source is one ESRI shapefile and the [layers](sources-pg-layers.md) made from it. Martin reads it with
the [`shapefile`](https://crates.io/crates/shapefile) crate.

```yaml
shapefile:
  sources:
    ocean:
      path: water-polygons-split-3857.zip
      layers:
        ocean:
          geometry: polygon
          attributes: []
```

```bash
martin cp --mode bulk --config config.yaml --source ocean --output-file ocean.mbtiles --max-zoom 8
```

`shapefile` and [`osm`](sources-osm.md) sources render together in one run, not with database sources.
In each tile, the layers of `shapefile` sources come first.
[Building a basemap from OpenStreetMap](recipe-osm-basemap.md) takes its `ocean` layer from one.

## Source keys

| Key                               | Meaning                                                                    |
|-----------------------------------|----------------------------------------------------------------------------|
| `path`                            | The `.shp` file, or a `.zip` file holding one                              |
| `layers`                          | Output layers, as for [table sources](sources-pg-layers.md)                |
| `minzoom`, `maxzoom`              | Narrow the zooms of `--min-zoom` and `--max-zoom`                          |
| `extent`, `buffer`, `clip_geom`   | Defaults of the layers, as for table sources                               |

## Files

| File     | Use                                                                                              |
|----------|--------------------------------------------------------------------------------------------------|
| `.shp`   | The geometries                                                                                   |
| `.shx`   | Where each record starts; without it, Martin reads the record headers of the `.shp` first        |
| `.dbf`   | The properties; required                                                                         |
| `.prj`   | The coordinate system: EPSG:4326 or EPSG:3857; required                                          |
| `.cpg`   | The text encoding of the `.dbf`; without it, the code page in the header of the `.dbf`           |

The files sit next to the `.shp`, with its name. A `.zip` file must hold exactly one `.shp`, in any folder. Martin
unpacks its parts into the first `--temp-dir` before the run, and deletes them when the run ends.

Martin does not reproject: a `.prj` that is neither WGS84 (EPSG:4326) nor Web Mercator (EPSG:3857, also as
ESRI's `WGS_1984_Web_Mercator_Auxiliary_Sphere`) is refused, and so is a shapefile without a `.prj`.

## Features

Each record is a feature, whose id is its record number, counting from 1. A record without a shape, or one marked
deleted in the `.dbf`, is left out.

| Shape type                    | Feature                             |
|-------------------------------|-------------------------------------|
| Point                         | point                               |
| MultiPoint                    | multipoint                          |
| PolyLine                      | line, or multiline if several parts |
| Polygon                       | polygon or multipolygon             |
| MultiPatch                    | refused                             |

Z and M values are dropped. A polygon's clockwise rings are outer rings, and each counter-clockwise ring is a hole
of the smallest outer ring around it, as GeoTools reads them; a hole in no outer ring is a polygon of its own. With
only one outer ring, every counter-clockwise ring is its hole, as in GeoTools.

Each field of the `.dbf` is a property, `null` where it is empty:

| Field type                                           | Property                                      |
|------------------------------------------------------|-----------------------------------------------|
| Character, Memo                                      | string                                        |
| Numeric without decimal places, Integer              | integer                                       |
| Numeric with decimal places, Float, Double, Currency | float                                         |
| Logical                                              | boolean                                       |
| Date, DateTime                                       | string, `2026-11-15` or `2026-11-15T09:30:00` |

Nothing is pushed down: Martin reads every record and checks `where` itself. With `--bbox`, it leaves out records
outside the tiles the box touches; without it, a file of the whole world, such as the ocean polygons, renders the
whole world.

## Draw order

Without `sort_by`, features keep the order of the records. The output does not depend on `--threads`.

## `martin test`

A case of a `shapefile` source is one record, with its record number as `record` in its input. A shapefile with a
field named `record` is refused.
