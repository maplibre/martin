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
[VersaTiles styles](https://github.com/versatiles-org/versatiles-style) draw it. The `osm` section is described in
[OpenStreetMap PBF Sources](sources-osm.md).

1. Download an extract, for example Berlin from [Geofabrik](https://download.geofabrik.de/):

    ```bash
    curl -LO https://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf
    ```

2. Save this as `basemap.yaml`, next to the extract:

    ??? example "basemap.yaml"

        ```yaml
        --8<-- "osm-basemap.yaml"
        ```

3. Generate the tileset:

    ```bash
    martin cp --mode bulk --config basemap.yaml --output-file berlin.mbtiles --format mvt --max-zoom 14
    ```

4. Serve it, and point a style's `versatiles-shortbread` source at `http://localhost:3000/berlin`:

    ```bash
    martin berlin.mbtiles
    ```

| Extract                     | PBF      | Features   | Tiles   | Time   | Memory  |
|-----------------------------|----------|------------|---------|--------|---------|
| Berlin, 2026-11-11          | 100 MB   | 1,530,231  | 915     | 4.3 s  | 2.2 GB  |
| Germany, 2026-11-11         | 4.9 GB   | 71,888,220 | 241,142 | 3m 15s | 9.5 GB  |

Measured on 24 threads, with the extract in the page cache.

## What it leaves out

- `ocean`, which Shortbread takes from coastline data outside OpenStreetMap, and the `boundary_labels`,
  `dam_*`, `pier_*`, `street_labels_points` and `streets_polygons_labels` layers.
- `boundaries` are the outlines of `type=boundary` relations as polygons, which line layers of a style draw, rather
  than Shortbread's lines with the lowest `admin_level` of the relations each way is in.
- Route relations, and tags of a relation copied to its member ways.
