---
description: Building a Shortbread basemap from an OpenStreetMap extract with martin cp --mode bulk
icon: material/map
tags:
  - openstreetmap
  - recipe
  - basemap
---

# Building a basemap from OpenStreetMap

This builds a basemap tileset from an [OpenStreetMap](https://www.openstreetmap.org/) extract, with the layers and
attributes of the [Shortbread](https://shortbread-tiles.org/) schema, so that
[VersaTiles styles](https://github.com/versatiles-org/versatiles-style) draw it. The configuration is a port of
Planetiler's [`shortbread.yml`](https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/src/main/resources/samples/shortbread.yml),
and Martin's tests render the 66 OpenStreetMap examples of its `shortbread.spec.yml`: 59 match, the others are
below. They also run a [`martin test`](martin-test.md) spec of it on Monaco. The `osm` section is described in [OpenStreetMap PBF Sources](sources-osm.md).

1. Download an extract, for example Berlin from [Geofabrik](https://download.geofabrik.de/):

    ```bash
    curl -LO https://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf
    ```

2. Save this as `shortbread.yaml`, next to the extract:

    ??? example "shortbread.yaml"

        ```yaml
        --8<-- "shortbread.yaml"
        ```

3. Generate the tileset:

    ```bash
    martin cp --mode bulk --config shortbread.yaml --output-file berlin.mbtiles --format mvt --max-zoom 14
    ```

4. Serve it, and point a style's `versatiles-shortbread` source at `http://localhost:3000/berlin`:

    ```bash
    martin berlin.mbtiles
    ```

| Extract                     | PBF      | Features   | Tiles   | Time   | Memory  |
|-----------------------------|----------|------------|---------|--------|---------|
| Berlin, 2026-11-11          | 100 MB   | 1,631,630  | 914     | 3.1 s  | 2.7 GB  |
| Germany, 2026-11-11         | 4.9 GB   | 75,134,826 | 228,018 | 3m 9s  | 18.2 GB |

Measured on 24 threads, with the extract in the page cache.

## How it differs from Planetiler

- `ocean` is left out: Shortbread takes it from coastline data outside OpenStreetMap.
- `way_area` is left out of `water_polygons`, `water_polygons_labels` and `boundary_labels`, and every
  `boundary_label` starts at zoom 5, where Planetiler starts the largest at zoom 2 to 4: expressions cannot read
  a feature's area.
- A closed way is a line or a polygon, as its tags make it an [area](sources-osm.md#features). Planetiler reads it
  as both unless it has `area=yes` or `area=no`, so its `street_polygons` also have closed `highway=pedestrian`
  ways, and `area:aeroway` ways, which are lines in no layer here.
- `boundary_labels` and `streets_polygons_labels` are at each polygon's centroid, and `pois`, `public_transport`
  and `addresses` at the centroid of each point, line or polygon. Planetiler takes points and a point on the
  surface of polygons, or for `addresses` the centroid of convex ones, and leaves lines out.
