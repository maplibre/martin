!!! warning "Not fully implemented"
    The bulk mode of `martin cp` is in the release binaries since 2.0.0-beta.6, but parts of what these
    pages describe are not implemented yet, and the configuration of `layers` may still change.

    - [x] Rendering points, lines and polygons, with fill ranges for covered tiles
    - [x] Tile grouping, MLT and MVT encoding, compression and deduplication
    - [x] Planning tables into layers and running a whole generation end to end
    - [x] The PostgreSQL source with partitioned whole-table scans
    - [x] Bulk mode in `martin cp`, and PMTiles output
    - [x] Layers with CEL expressions, `rules` and `sort_by`
    - [ ] OSM PBF, shapefile, GeoPackage and CSV files as sources
    - [ ] `geometry` other than `point`, `line` and `polygon`: Planetiler's `polygon_centroid`,
      `line_centroid`, `line_midpoint`, `centroid`, `polygon_centroid_if_convex`, `polygon_point_on_surface`,
      `point_on_line` and `innermost_point`
    - [x] [Per-tile operations](sources-pg-layers.md#per-tile-operations) (`thinning`,
      `merge_multi`, `limit`)
