!!! warning "Not fully implemented"
    The bulk mode of `martin cp` is in the release binaries since 2.0.0-beta.6, but parts of what these
    pages describe are not implemented yet, and the configuration of `layers` may still change.

    - [x] Rendering points, lines and polygons, with fill ranges for covered tiles
    - [x] Tile grouping, MLT and MVT encoding, compression and deduplication
    - [x] Planning tables into layers and running a whole generation end to end
    - [x] The PostgreSQL source with partitioned whole-table scans
    - [x] Bulk mode in `martin cp`, and PMTiles output
    - [x] Layers with CEL expressions, `rules` and `sort_by`
    - [x] [OSM PBF files](sources-osm.md) and [shapefiles](sources-shapefile.md) as sources
    - [ ] GeoPackage and CSV files as sources
    - [x] `geometry` with Planetiler's `polygon_centroid`, `line_centroid`, `centroid`,
      `polygon_point_on_surface` and `polygon_centroid_if_convex`
    - [ ] `geometry` with Planetiler's `line_midpoint`, `point_on_line` and `innermost_point`
    - [x] [Per-tile operations](sources-pg-layers.md#per-tile-operations) (`merge_lines`,
      `merge_polygons`, `thinning`, `merge_multi`, `limit`)
