---
description: Building a Shortbread basemap from an OpenStreetMap extract with martin cp --mode bulk
icon: material/map
tags:
  - openstreetmap
  - recipe
  - basemap
---

# Building a basemap from OpenStreetMap

This builds a basemap tileset from an [OpenStreetMap](https://www.openstreetmap.org/) extract, with the layers and attributes using the [Shortbread](https://shortbread-tiles.org/) schema, so that [VersaTiles styles](https://github.com/versatiles-org/versatiles-style) can draw them.
The configuration is a port of Planetiler's [`shortbread.yml`](https://github.com/onthegomap/planetiler/blob/main/planetiler-custommap/src/main/resources/samples/shortbread.yml), and Martin's tests use the 67 examples of its `shortbread.spec.yml`.

!!! important "More information on the config used"
    The `osm` section is described in [OpenStreetMap PBF Sources](sources-osm.md), the `shapefile` section in [Shapefile Sources](sources-shapefile.md).

1. Download an extract, for example Berlin from [Geofabrik](https://download.geofabrik.de/), and the ocean
   polygons of the whole world from [osmdata.openstreetmap.de](https://osmdata.openstreetmap.de/data/water-polygons.html),
   about 900 MB:

    ```bash
    curl -LO https://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf
    curl -LO https://osmdata.openstreetmap.de/download/water-polygons-split-3857.zip
    ```

2. Save this as `shortbread.yaml`, next to both downloads:

    ??? example "shortbread.yaml"

        ```yaml
        --8<-- "shortbread.yaml"
        ```

3. Generate the tileset of the extract's area. The ocean polygons cover the whole world, so without `--bbox` the
   tileset does too:

    ```bash
    martin cp --mode bulk --config shortbread.yaml --output-file berlin.mbtiles --format mvt --max-zoom 14 \
      --bbox 13.08,52.33,13.77,52.68
    ```

4. Serve it, and point a style's `versatiles-shortbread` source at `http://localhost:3000/berlin`:

    ```bash
    martin berlin.mbtiles
    ```

| Extract                     | PBF      | Features   | Tiles   | Time   | Memory  |
|-----------------------------|----------|------------|---------|--------|---------|
| Berlin, 2026-11-11          | 100 MB   | 1,646,140  | 889     | 15.3 s | 6.0 GB  |
| Germany, 2026-11-11         | 4.9 GB   | 75,149,336 | 251,613 | 2m 54s | 19.1 GB |

Measured on 24 threads, with the extract and the ocean polygons of 2026-11-14 in the page cache, each run with the
`--bbox` of its extract. 3 s of each run unpack the ocean polygons. With `--bbox`, Martin keeps whole tiles, so the
ocean polygons of the whole world go into the low-zoom tiles: they take Berlin from 3 s to 15 s.
