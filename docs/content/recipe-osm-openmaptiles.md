---
description: Building an OpenMapTiles basemap from an OpenStreetMap extract with martin cp --mode bulk
icon: material/map
tags:
  - openstreetmap
  - recipe
  - basemap
---

# Building OpenMapTiles from OpenStreetMap

This builds a basemap tileset from an [OpenStreetMap](https://www.openstreetmap.org/) extract with the 16 layers of
the [OpenMapTiles schema](https://openmaptiles.org/schema/), so that OpenMapTiles styles such as
[OSM Bright](https://github.com/openmaptiles/osm-bright-gl-style) and
[Positron](https://github.com/openmaptiles/positron-gl-style) draw it. The configuration is a port of
[Planetiler's OpenMapTiles profile](https://github.com/openmaptiles/planetiler-openmaptiles), with its zooms,
minimum sizes, buffers, label grids, merges and draw orders. It reads OpenStreetMap only, plus the ocean polygons of
the whole world, and leaves out what Planetiler takes from Natural Earth and Wikidata, see
[How it differs](#how-it-differs-from-planetilers-openmaptiles).

Martin's tests render the inputs of Planetiler's OpenMapTiles layer tests: 229 of its 341 examples match. The others
are differences listed in [How it differs](#how-it-differs-from-planetilers-openmaptiles), or what the tests check
differently from a rendered tile:

| Examples | Why they do not match                                                                                   |
|---------:|---------------------------------------------------------------------------------------------------------|
| 33       | Natural Earth and lake centerline inputs, which are not read                                             |
| 16       | Country borders with `adm0_l`/`adm0_r`, which Planetiler draws after reading the file, outside the tested code |
| 16       | Natural Earth ranks of countries, states, cities and seas                                                |
| 11       | `rank` from the label grid, which Planetiler's tests leave out and Martin's tiles have                   |
| 8        | GB and IE road networks                                                                                  |
| 6        | `route_N` order and duplicates                                                                           |
| 5        | Feature lengths: short trunks at z5, `brunnel` by size                                                   |
| 5        | Ocean polygons and large lakes below z6, and inputs the tests' whole-world polygons were shrunk to 1° for |
| 4        | Wikidata translations, translated names of river relations                                               |
| 4        | `--boundary-osm-only` zooms, `hide_3d`, `customary_ft`                                                   |
| 3        | A minimum size dropping a feature below the zoom Planetiler declares, an attribute asked below the feature's zoom |
| 1        | A closed `man_made=pier` way drawn as a line too                                                         |

The tests are generated from Planetiler's own by
[`openmaptiles_spec.py`](https://github.com/maplibre/martin/blob/main/tests/fixtures/osm/openmaptiles_spec.py).
The `osm` section is described in [OpenStreetMap PBF Sources](sources-osm.md), the `shapefile` section in
[Shapefile Sources](sources-shapefile.md), and the layer keys in [Table Source Layers](sources-pg-layers.md).

1. Download an extract, for example Berlin from [Geofabrik](https://download.geofabrik.de/), and the ocean
   polygons of the whole world from [osmdata.openstreetmap.de](https://osmdata.openstreetmap.de/data/water-polygons.html),
   about 900 MB:

    ```bash
    curl -LO https://download.geofabrik.de/europe/germany/berlin-latest.osm.pbf
    curl -LO https://osmdata.openstreetmap.de/download/water-polygons-split-3857.zip
    ```

2. Save this as `openmaptiles.yaml`, next to both downloads:

    ??? example "openmaptiles.yaml"

        ```yaml
        --8<-- "openmaptiles.yaml"
        ```

3. Generate the tileset of the extract's area:

    ```bash
    martin cp --mode bulk --config openmaptiles.yaml --output-file berlin.mbtiles --format mvt --max-zoom 14 \
      --bbox 13.08,52.33,13.77,52.68
    ```

4. Serve it, and point the `openmaptiles` source of a style at `http://localhost:3000/berlin`:

    ```bash
    martin berlin.mbtiles
    ```

| Run                                  | PBF    | Features  | Tiles | Size    | Time   | Memory |
|--------------------------------------|--------|-----------|-------|---------|--------|--------|
| Martin, Berlin, 2026-11-11           | 100 MB | 1,724,829 | 912   | 45.4 MB | 1m 8s  | 8.4 GB |
| Planetiler 0.10.2, the same extract  | 100 MB | -         | 942   | 41.8 MB | 1m 19s | 5.0 GB |

Measured on 24 threads, with the extract and the ocean polygons in the page cache. Martin unpacks the ocean polygons
in 6 s and keeps whole tiles of the `--bbox`. Planetiler's time includes 23 s of Natural Earth and lake centerline
downloads and 15 s reading the ocean polygons, and it covers the extract's own bounds.

## How it differs from Planetiler's OpenMapTiles

The configuration reads OpenStreetMap and the ocean polygons only. Planetiler also reads Natural Earth, the lake
centerlines and Wikidata, and runs code that `martin cp` has no equivalent for yet:

| Layer                 | Planetiler                                                                | This configuration                                                                 |
|-----------------------|---------------------------------------------------------------------------|------------------------------------------------------------------------------------|
| all                   | Wikidata translations and transliterated `name:latin`                    | Only the `name:xx` tags of the 81 OpenMapTiles languages                          |
| `water`               | Natural Earth ocean and lakes at z0-5, ocean polygons from z6             | Ocean polygons from z0; OpenStreetMap lakes of 25 km² and more at z4-5 (2000 km² from z2, 20000 km² from z0) |
| `water`               | A Natural Earth lake's `id` is the OpenStreetMap lake it overlaps most    | Large lakes keep their own `id` at z0-5                                            |
| `waterway`            | Natural Earth rivers at z3-5; river relation members at z6-8 only when the whole river is long enough | No rivers below z6; at z6-8 every member of a named `waterway=river` relation, merged and kept from 1 px |
| `waterway`            | Relation names in all languages, members with role `main_stream` or none | Relation `name`, `name_en`, `name_de`, `name:latin`, `name:nonlatin`, `name_int`; any role |
| `landcover`           | Natural Earth glaciers and ice shelves at z0-6                            | From z7 only                                                                       |
| `landcover`           | Merges wood and forest of fewer than 300 vertices at z8-13                | Merges all wood and forest at z8-13, as there is no vertex count                   |
| `landuse`             | Natural Earth urban areas at z4-5                                         | From z6 only                                                                       |
| `mountain_peak`       | `customary_ft` inside the United States; no `rank` at z14 beyond the grid | No `customary_ft`; `rank` from the z0-13 grid only                                 |
| `boundary`            | Natural Earth lines at z0-4, OpenStreetMap from z4-5                       | OpenStreetMap from z0, as Planetiler's `--boundary-osm-only`                        |
| `boundary`            | `adm0_l` and `adm0_r` from the country polygons left and right of each border | No `adm0_l` and `adm0_r`                                                     |
| `boundary`            | Borders joined across ways by a line merger before tiling                 | Joined per tile by `merge_lines`                                                   |
| `place`               | Country, state and city ranks from Natural Earth polygons and points nearby | Country ranks from Natural Earth by ISO code (`openmaptiles_ranks.py`), every state and province with rank 6, city ranks from `population` and `capital` |
| `transportation`      | `network` and z4-5 roads from GB and IE `ref` patterns inside Natural Earth country polygons | US and Canadian networks from route relations only                 |
| `transportation`      | Trunk segments shorter than 500 m at z5, as `motorway` up to z5            | Trunk from z6 unless a route relation upgrades it                                  |
| `transportation`      | `brunnel` and `layer` from z4 and z9 on segments longer than 4 px          | `brunnel` and `layer` from z12                                                     |
| `transportation`      | Closed `man_made=pier` ways are only polygons                             | Also drawn as `pier` lines                                                         |
| `transportation_name` | `route_N` sorted by network type, network and `ref`, without duplicates   | `route_1` to `route_6` in relation id order, duplicates kept                        |
| `transportation_name` | `motorway_junction` class from the highest road through the node           | `class` is always `motorway`                                                       |
| `transportation_name` | z12-13 lines shorter than 6 px per letter of the name are dropped         | z12-13 merged lines shorter than 24 px are dropped                                 |
| `building`            | `hide_3d` from `type=building` relations                                  | No `hide_3d`                                                                       |
| `water_name`          | Lake centerlines as labels; Natural Earth ranks of seas                   | Points only; sea ranks from the `rank` tag                                         |
| `poi`                 | `agg_stop` picks one stop of each group sharing a `uic_ref`                | No `agg_stop`                                                                      |
| point layers          | `buffer` of 256 px                                                         | 127.9 px, the most an `extent` of 4096 allows                                      |
| point layers          | Points from polygons dropped while the polygon is below a minimum size (university, lake label) | The same as a `minzoom` from the polygon's area                 |
| `poi`, `place`        | Draw order by class rank and Natural Earth rank                            | Class rank from the tags, without telling apart keys of one object                  |
| `building`            | Heights in any unit Planetiler parses                                      | Meters, kilometers, feet and inches, miles and nautical miles                       |
| all                   | `min_size` compares the square root of a polygon's area and a line's length | Compares the larger side of the bounding box, so more thin polygons and lines stay |
| `landuse`             | Residential areas closer than 0.2 px joined at z9-12                       | Closer than 0.1 px, as larger distances could not make a valid union in Berlin      |

The configuration uses a few constructs worth knowing when changing it:

- A `minzoom` above a layer's zooms is clamped, so zoom bands are separate layers writing into one output layer
  with [`layer`](sources-pg-layers.md#combining-layers), each with its own `min_size` and `buffer`.
- A `where` with no equivalent for the PBF read, such as one calling `area()` or a regular expression, makes Martin
  read every object. Such conditions are a `minzoom` that returns 14 and a `maxzoom` that returns 0 instead, which
  leaves the feature out.
- A hidden `_unmerged: osm_id` keeps a feature out of a merge, as Planetiler's selective merges do.
